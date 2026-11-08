//! Attachments and Hawk signing through the state and the send path, against
//! an in-process server. Ported from the host's `http.rs` tests with their
//! meaning unchanged.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::Router;
use axum::http::HeaderMap;
use axum::routing::{any, post};
use serde_json::{Value, json};
use tokio::runtime::Runtime;

use crate::config::Api;
use crate::reply::Reply;
use crate::send;
use crate::state::{Exchange, Instance, State};
use crate::upload::FilePart;

/// The servers live on their own runtime, apart from the plugin's: a send
/// blocks the test thread on the plugin's.
static SERVERS: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().expect("runtime"));

fn serve(app: Router) -> String {
    SERVERS.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        format!("http://{address}/")
    })
}

/// Echoes what the server received: every Content-Type and the raw body,
/// lossily decoded (the fixtures are ASCII).
fn multipart_echo() -> String {
    serve(Router::new().route(
        "/upload",
        any(|headers: HeaderMap, body: axum::body::Bytes| async move {
            axum::Json(json!({
                "content_types": headers
                    .get_all("content-type")
                    .iter()
                    .map(|v| v.to_str().unwrap_or("").to_string())
                    .collect::<Vec<_>>(),
                "body": String::from_utf8_lossy(&body),
            }))
        }),
    ))
}

fn state_for(base: &str, default_headers: &Value) -> State {
    let api =
        Api::parse(&json!({"base_url": base, "default_headers": default_headers})).expect("valid");
    let mut state = State::new(BTreeMap::from([(
        "main".to_string(),
        Instance::Api(Arc::new(api)),
    )]));
    state.begin("main").expect("selected");
    state
}

fn part(name: &str, file_name: &str, bytes: &[u8]) -> FilePart {
    FilePart {
        name: name.to_string(),
        file_name: file_name.to_string(),
        mime: mime_guess::from_path(file_name)
            .first_or_octet_stream()
            .to_string(),
        bytes: bytes.into(),
    }
}

/// A send, the way the step does it: what it made is kept in the state.
fn send_it(state: &mut State, path: &str) -> Reply {
    let (recipe, api) = state.begin_send("POST", path).expect("begin");
    let (reply, exchange) = send::send(&recipe.api, &recipe, &api, false);
    if let Some(exchange) = exchange {
        state.recipe = Some(recipe);
        state.exchange = Some(exchange);
    }
    reply
}

/// The same request again, as a polled assertion does from its second attempt.
fn replay(state: &mut State) {
    let recipe = state.recipe.clone().expect("a request was sent");
    let api = state.api_named(&recipe.api).expect("api");
    let (_, exchange) = send::send(&recipe.api, &recipe, &api, false);
    state.exchange = exchange;
}

fn echoed(state: &State) -> Value {
    let exchange = state.exchange.as_ref().expect("an exchange");
    serde_json::from_str(&exchange.response_body).expect("JSON")
}

fn body_of(state: &State) -> String {
    echoed(state)["body"].as_str().expect("body").to_string()
}

#[test]
fn an_attached_file_is_sent_as_a_multipart_part_with_its_name_type_and_bytes() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.attach(part("content", "id_front.png", b"PNGDATA"));
    assert!(send_it(&mut state, "/upload").failure().is_none());
    let body = body_of(&state);
    assert!(
        body.contains(r#"name="content"; filename="id_front.png""#),
        "{body}"
    );
    assert!(body.contains("Content-Type: image/png"), "{body}");
    assert!(body.contains("PNGDATA"), "{body}");
}

#[test]
fn form_parameters_travel_as_text_parts_beside_a_file() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.set_form(vec![("metadata".into(), r#"{"a":1}"#.into())]);
    state.attach(part("content", "x.bin", b"B"));
    send_it(&mut state, "/upload");
    let body = body_of(&state);
    assert!(body.contains(r#"name="metadata""#), "{body}");
    assert!(body.contains(r#"{"a":1}"#), "{body}");
}

#[test]
fn one_part_name_can_carry_several_files() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.attach(part("docs", "a.txt", b"AAA"));
    state.attach(part("docs", "b.txt", b"BBB"));
    send_it(&mut state, "/upload");
    assert_eq!(body_of(&state).matches(r#"name="docs""#).count(), 2);
}

#[test]
fn a_file_with_an_unknown_extension_is_octet_stream() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.attach(part("content", "blob.zzzunknown", b"B"));
    send_it(&mut state, "/upload");
    let body = body_of(&state);
    assert!(
        body.contains("Content-Type: application/octet-stream"),
        "{body}"
    );
}

#[test]
fn without_an_attachment_form_parameters_stay_urlencoded() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.set_form(vec![("a".into(), "1".into())]);
    send_it(&mut state, "/upload");
    assert_eq!(
        echoed(&state)["content_types"],
        json!(["application/x-www-form-urlencoded"])
    );
}

#[test]
fn the_multipart_content_type_wins_over_every_configured_one() {
    let defaults = json!({"Content-Type": "application/json"});
    let mut state = state_for(&multipart_echo(), &defaults);
    state.set_header("content-type", "text/plain");
    state.add_header("CONTENT-TYPE", "application/xml");
    state.attach(part("content", "x.bin", b"B"));
    send_it(&mut state, "/upload");
    let echoed = echoed(&state);
    let types = echoed["content_types"].as_array().expect("array");
    assert_eq!(types.len(), 1, "{types:?}");
    let sent = types[0].as_str().expect("string");
    assert!(sent.starts_with("multipart/form-data; boundary="), "{sent}");
    // The recorded exchange shows the real boundary, once, after the others.
    let exchange: &Exchange = state.exchange.as_ref().expect("exchange");
    let recorded: Vec<_> = exchange
        .request_headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0].1, sent);
}

#[test]
fn a_replay_resends_the_same_file_content() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    state.attach(part("content", "x.bin", b"ORIGINAL"));
    send_it(&mut state, "/upload");
    state.attach(part("content", "x.bin", b"MUTATED"));
    replay(&mut state);
    let body = body_of(&state);
    assert!(
        body.contains("ORIGINAL") && !body.contains("MUTATED"),
        "{body}"
    );
}

#[test]
fn the_recorded_request_lists_parts_and_never_the_file_bytes() {
    let mut state = state_for(&multipart_echo(), &json!({}));
    // A text value longer than the dump's body limit must not push the file
    // lines out of it.
    let long = "m".repeat(2000);
    state.set_form(vec![("meta".into(), long.clone())]);
    state.attach(part("content", "id_front.png", b"SECRETBYTES"));
    send_it(&mut state, "/upload");
    let exchange = state.exchange.as_ref().expect("exchange");
    let sent = exchange.request_body.as_deref().expect("a recorded body");
    assert!(
        sent.contains("content: id_front.png (image/png, 11 bytes)"),
        "{sent}"
    );
    assert!(sent.contains("meta=mmm"), "{sent}");
    assert!(!sent.contains(&long), "{sent}");
    assert!(!sent.contains("SECRETBYTES"), "{sent}");
    let diagnostic = send::diagnostic(exchange);
    assert_eq!(diagnostic["content"]["request_form"], "multipart");
}

#[test]
fn attachments_are_cleared_by_reset_api_switch_and_a_raw_body() {
    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.attach(part("content", "x.bin", b"B"));
    state.reset();
    assert!(state.pending.files.is_empty(), "reset");

    state.begin("main").expect("selected");
    state.attach(part("content", "x.bin", b"B"));
    state.set_body(Some("{}".into()));
    assert!(state.pending.files.is_empty(), "a raw body");

    state.attach(part("content", "x.bin", b"B"));
    state.set_body(None);
    assert!(state.pending.files.is_empty(), "an empty body");

    state.attach(part("content", "x.bin", b"B"));
    state.switch_to("main").expect("declared");
    assert!(state.pending.files.is_empty(), "an API switch");
}

#[test]
fn attaching_a_file_drops_a_pending_raw_body_and_keeps_form_parameters() {
    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.set_body(Some("{}".into()));
    state.attach(part("content", "x.bin", b"B"));
    assert!(state.pending.body.is_none());
    state.set_form(vec![("a".into(), "1".into())]);
    assert_eq!(state.pending.files.len(), 1, "a form must not drop files");
}

#[test]
fn a_signed_multipart_or_form_request_fails_before_transport() {
    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.attach(part("content", "x.bin", b"B"));
    state.sign_next("id", "key");
    let reply = send_it(&mut state, "/x");
    let (class, error) = reply.failure().expect("a failure");
    assert_eq!(class, crate::reply::Class::Input);
    assert!(error.contains("multipart"), "{error}");

    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.set_form(vec![("name".into(), "value".into())]);
    state.sign_next("id", "key");
    let reply = send_it(&mut state, "/x");
    let (class, error) = reply.failure().expect("a failure");
    assert_eq!(class, crate::reply::Class::Input);
    assert!(error.contains("form"), "{error}");
}

#[test]
fn signing_is_cleared_by_an_api_switch_and_by_the_scenario_boundary() {
    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.sign_next("id", "key");
    state.switch_to("main").expect("declared");
    assert!(state.pending.signer.is_none(), "switch");
    state.sign_next("id", "key");
    state.reset();
    assert!(state.pending.signer.is_none(), "reset");
}

#[test]
fn signing_is_one_shot_and_the_recipe_keeps_it_for_replays() {
    let mut state = state_for("http://127.0.0.1:1/", &json!({}));
    state.sign_next("id", "key");
    let (first, _) = state.begin_send("POST", "/x").expect("begin");
    assert!(first.signer.is_some() && state.pending.signer.is_none());
    let (second, _) = state.begin_send("POST", "/x").expect("begin");
    assert!(second.signer.is_none(), "the next send goes out unsigned");
}

#[test]
fn a_replay_generates_a_fresh_hawk_authorization() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = Arc::clone(&seen);
    let base = serve(Router::new().route(
        "/hawk",
        post(move |headers: HeaderMap| {
            let captured = Arc::clone(&captured);
            async move {
                captured.lock().expect("lock").push(
                    headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string(),
                );
                axum::Json(json!({"ok": true}))
            }
        }),
    ));
    let mut state = state_for(&base, &json!({}));
    state.set_header("content-type", "application/json");
    state.set_body(Some(r#"{"request":"saved"}"#.into()));
    state.sign_next("session", "secret");
    assert!(send_it(&mut state, "/hawk").failure().is_none());
    replay(&mut state);
    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 2);
    assert!(
        seen.iter()
            .all(|value| value.starts_with("Hawk id=\"session\""))
    );
    assert_ne!(seen[0], seen[1]);
}

#[test]
fn a_signed_header_joins_the_secrets_of_the_reply() {
    let base = serve(Router::new().route("/hawk", post(|| async { axum::Json(json!({})) })));
    let mut state = state_for(&base, &json!({}));
    state.sign_next("session", "secret");
    let reply = send_it(&mut state, "/hawk");
    let sent: Value = serde_json::from_str(&reply.json()).expect("JSON");
    assert!(
        sent["secrets"].to_string().contains("Hawk id="),
        "the default sensitive names include authorization: {sent}"
    );
}
