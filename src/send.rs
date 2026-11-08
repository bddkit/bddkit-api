//! Sending one request: what it registers with the host before it leaves, the
//! exchange it makes, and what the response registers on the way back. The
//! plugin sends real values and says what is sensitive; the host masks.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::{Client, Method, Request, Url};
use serde_json::{Value, json};

use crate::config::Api;
use crate::header;
use crate::reply::{Class, Reply};
use crate::runtime;
use crate::state::{BodyForm, Exchange, Pairs, Recipe};

/// One client per resource, built on first use and kept for the run: the
/// timeout only, no cookie store, reqwest's default redirect policy. Keyed by
/// the timeout too, so an in-process test that reuses a name cannot get a
/// client built for another config.
static CLIENTS: LazyLock<Mutex<HashMap<(String, u64), Client>>> = LazyLock::new(Mutex::default);

fn client(name: &str, api: &Api) -> Result<Client, String> {
    // Nothing under this lock can panic, and the map holds no half-updated state.
    let mut clients = CLIENTS.lock().unwrap_or_else(PoisonError::into_inner);
    let key = (name.to_string(), api.timeout_secs);
    if let Some(client) = clients.get(&key) {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(api.timeout_secs))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {e}"))?;
    clients.insert(key, client.clone());
    Ok(client)
}

/// The request resolved against its resource.
struct Prepared {
    url: Url,
    method: Method,
    /// The body as it travels: the form joined, or the raw body.
    body: Option<String>,
    /// What the path itself carried after a `?`.
    literal_query: Pairs,
}

/// The pairs a path writes after its `?`, read on their own so that a path
/// `join` refuses still registers them before any error is built.
fn path_query(path: &str) -> Pairs {
    let path = path.split('#').next().unwrap_or_default();
    let Some((_, query)) = path.split_once('?') else {
        return Vec::new();
    };
    Url::parse(&format!("http://x/?{query}")).map_or_else(
        |_| Vec::new(),
        |url| {
            url.query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        },
    )
}

fn prepare(recipe: &Recipe, api: &Api) -> Result<Prepared, String> {
    let mut url = api
        .base_url
        .join(&recipe.path)
        .map_err(|e| format!("invalid path: {e}"))?;
    let literal_query = path_query(&recipe.path);
    if !recipe.query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in &recipe.query {
            pairs.append_pair(k, v);
        }
    }
    let method = Method::from_bytes(recipe.method.as_bytes())
        .map_err(|e| format!("invalid HTTP method {:?}: {e}", recipe.method))?;
    let body = match &recipe.form {
        Some(form) => Some(
            form.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        ),
        None => recipe.body.clone(),
    };
    Ok(Prepared {
        url,
        method,
        body,
        literal_query,
    })
}

/// What the request carries under a sensitive name, registered before it
/// leaves: the values of sensitive headers, the JSON body, the form fields and
/// the query fields (a literal `?token=` in the path included), with raw pairs
/// so a value holding `&`, `=`, `+` or `%XX` is registered whole.
fn registered(recipe: &Recipe, api: &Api, literal_query: &[(String, String)]) -> Reply {
    let mut reply = Reply::passed();
    for (name, value) in &recipe.headers {
        if is_sensitive(api, name) {
            reply.secrets.extend(header::secret_values(name, value));
        }
    }
    let query: Pairs = literal_query.iter().chain(&recipe.query).cloned().collect();
    if !query.is_empty() {
        reply.scan.push(json!({"kind": "query", "pairs": query}));
    }
    if let Some(form) = &recipe.form {
        reply.scan.push(json!({"kind": "form", "pairs": form}));
    }
    if let Some(body) = &recipe.body {
        reply.scan.push(json!({"kind": "json", "content": body}));
    }
    reply
}

fn is_sensitive(api: &Api, name: &str) -> bool {
    api.sensitive_headers
        .iter()
        .any(|masked| masked.eq_ignore_ascii_case(name))
}

fn build(client: &Client, recipe: &Recipe, prepared: &Prepared) -> Result<Request, String> {
    let mut builder = client.request(prepared.method.clone(), prepared.url.clone());
    for (k, v) in &recipe.headers {
        builder = builder.header(k, v);
    }
    if let Some(form) = &recipe.form {
        builder = builder.form(form);
    } else if let Some(body) = &recipe.body {
        builder = builder.body(body.clone());
    }
    builder
        .build()
        .map_err(|e| format!("failed to build HTTP request: {e}"))
}

/// What came back.
struct Answer {
    status: u16,
    headers: Pairs,
    body: String,
}

/// The method and the URL without its query string, which `reqwest`'s own
/// text leaves out: the plugin cannot position-mask a URL as the host can.
fn describe(prepared: &Prepared) -> String {
    let mut url = prepared.url.clone();
    url.set_query(None);
    url.set_fragment(None);
    // The host cannot position-mask what the plugin builds: no credentials.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    format!("{} {url}", prepared.method)
}

fn exchange_with(client: &Client, request: Request, prepared: &Prepared) -> Result<Answer, String> {
    let what = describe(prepared);
    runtime::block_on(async {
        let response = client
            .execute(request)
            .await
            .map_err(|e| format!("{what}: request failed: {}", e.without_url()))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-UTF8>").to_string(),
                )
            })
            .collect();
        let body = response
            .text()
            .await
            .map_err(|e| format!("{what}: failed to read response body: {}", e.without_url()))?;
        Ok(Answer {
            status,
            headers,
            body,
        })
    })
}

/// The response's half of the registrations, on every status: the body when it
/// parses as JSON, and the values of the headers under sensitive names.
fn register_answer(reply: &mut Reply, api: &Api, answer: &Answer) {
    if serde_json::from_str::<Value>(&answer.body).is_ok() {
        reply
            .scan
            .push(json!({"kind": "json", "content": answer.body}));
    }
    for (name, value) in &answer.headers {
        if is_sensitive(api, name) {
            reply.secrets.extend(header::secret_values(name, value));
        }
    }
}

/// The exchange as the one `http` diagnostic the host renders.
pub fn diagnostic(exchange: &Exchange) -> Value {
    json!({
        "kind": "http",
        "resource": exchange.api,
        "content": {
            "method": exchange.method,
            "url": exchange.url,
            "request": {"headers": exchange.request_headers, "body": exchange.request_body},
            "response": {"headers": exchange.response_headers, "body": exchange.response_body},
            "status": exchange.status,
            "duration_us": exchange.duration_us,
            "sensitive_headers": exchange.sensitive_headers,
            "request_form": exchange.request_form.as_str(),
        },
    })
}

/// Sends the request. The exchange comes back beside the reply when there is
/// one; a request that never got an answer has none, and its reply carries no
/// diagnostic.
pub fn send(name: &str, recipe: &Recipe, api: &Api, evidence: bool) -> (Reply, Option<Exchange>) {
    let prepared = match prepare(recipe, api) {
        Ok(prepared) => prepared,
        Err(error) => {
            let reply = registered(recipe, api, &path_query(&recipe.path));
            return (reply.into_failure(Class::Input, error), None);
        }
    };
    let mut reply = registered(recipe, api, &prepared.literal_query);
    let client = match client(name, api) {
        Ok(client) => client,
        Err(error) => return (reply.into_failure(Class::Input, error), None),
    };
    let request = match build(&client, recipe, &prepared) {
        Ok(request) => request,
        Err(error) => return (reply.into_failure(Class::Input, error), None),
    };
    let request_headers = recipe.headers.clone();
    let started = Instant::now();
    let answer = match exchange_with(&client, request, &prepared) {
        Ok(answer) => answer,
        Err(error) => return (reply.into_failure(Class::Transport, error), None),
    };
    register_answer(&mut reply, api, &answer);
    let exchange = Exchange {
        method: prepared.method.to_string(),
        url: prepared.url.to_string(),
        request_headers,
        request_body: prepared.body,
        request_form: if recipe.form.is_some() {
            BodyForm::Form
        } else {
            BodyForm::Raw
        },
        status: answer.status,
        response_headers: answer.headers,
        response_body: answer.body,
        duration_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        api: recipe.api.clone(),
        sensitive_headers: api.sensitive_headers.clone(),
    };
    if evidence {
        reply.diagnostics.push(diagnostic(&exchange));
    }
    (reply, Some(exchange))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(extra: &Value) -> Api {
        let mut config = json!({"base_url": "http://127.0.0.1:1/"});
        if let (Some(c), Some(e)) = (config.as_object_mut(), extra.as_object()) {
            c.extend(e.clone());
        }
        Api::parse(&config).expect("valid")
    }

    fn recipe() -> Recipe {
        Recipe {
            api: "main".into(),
            method: "POST".into(),
            path: "/p?api_token=a%26b".into(),
            query: vec![("q".into(), "1".into())],
            headers: vec![
                ("Authorization".into(), "Bearer abc1234".into()),
                ("X-Plain".into(), "visible".into()),
            ],
            body: Some(r#"{"token":"t"}"#.into()),
            form: None,
        }
    }

    #[test]
    fn the_path_query_is_registered_decoded_and_whole() {
        let api = api(&json!({}));
        let prepared = prepare(&recipe(), &api).expect("ok");
        assert_eq!(
            prepared.literal_query,
            [("api_token".to_string(), "a&b".to_string())]
        );
        assert_eq!(prepared.url.query(), Some("api_token=a%26b&q=1"));
    }

    #[test]
    fn a_path_the_url_parser_refuses_still_registers_its_query_and_quotes_none_of_it() {
        let api = api(&json!({}));
        let mut r = recipe();
        r.path = "//host:99999/x?api_token=shortsecret".into();
        let (reply, _) = send("unit-bad-path", &r, &api, false);
        let v: Value = serde_json::from_str(&reply.json()).expect("JSON");
        assert_eq!(v["class"], "input");
        assert!(!v["error"].as_str().expect("error").contains("shortsecret"));
        assert!(v["scan"].to_string().contains("api_token"), "{v}");
    }

    #[test]
    fn the_transport_error_carries_no_credentials_of_the_base_url() {
        let api = Api::parse(&json!({"base_url": "http://svc:pw12@127.0.0.1:1/"})).expect("valid");
        let (reply, _) = send("unit-userinfo", &recipe(), &api, false);
        let v: Value = serde_json::from_str(&reply.json()).expect("JSON");
        assert!(!v["error"].as_str().expect("error").contains("pw12"), "{v}");
    }

    #[test]
    fn what_the_request_carries_is_registered_before_it_leaves() {
        let api = api(&json!({}));
        let prepared = prepare(&recipe(), &api).expect("ok");
        let reply = registered(&recipe(), &api, &prepared.literal_query);
        assert_eq!(reply.secrets, ["Bearer abc1234", "abc1234"]);
        assert_eq!(
            reply.scan,
            [
                json!({"kind": "query", "pairs": [["api_token", "a&b"], ["q", "1"]]}),
                json!({"kind": "json", "content": r#"{"token":"t"}"#}),
            ]
        );
    }

    #[test]
    fn form_fields_are_registered_as_pairs_and_the_body_joined() {
        let api = api(&json!({}));
        let mut r = recipe();
        r.body = None;
        r.form = Some(vec![("password".into(), "a=b&c".into())]);
        let prepared = prepare(&r, &api).expect("ok");
        assert_eq!(prepared.body.as_deref(), Some("password=a=b&c"));
        let reply = registered(&r, &api, &prepared.literal_query);
        assert!(
            reply
                .scan
                .contains(&json!({"kind": "form", "pairs": [["password", "a=b&c"]]}))
        );
    }

    #[test]
    fn a_header_is_secret_by_the_resources_own_list() {
        let only = api(&json!({"sensitive_headers_only": ["x-plain"]}));
        let reply = registered(&recipe(), &only, &[]);
        assert_eq!(reply.secrets, ["visible"]);
    }

    #[test]
    fn a_bad_path_or_method_is_refused_before_anything_is_sent() {
        let api = api(&json!({}));
        let mut r = recipe();
        r.method = "BAD METHOD".into();
        assert!(prepare(&r, &api).is_err_and(|e| e.contains("invalid HTTP method")));
    }

    #[test]
    fn the_response_registers_json_and_sensitive_headers_on_any_status() {
        let api = api(&json!({}));
        let answer = Answer {
            status: 500,
            headers: vec![("Set-Cookie".into(), "sid=zzz9999; Path=/".into())],
            body: r#"{"jwt":"tok-abc"}"#.into(),
        };
        let mut reply = Reply::passed();
        register_answer(&mut reply, &api, &answer);
        assert_eq!(reply.secrets, ["sid=zzz9999; Path=/", "zzz9999"]);
        assert_eq!(reply.scan.len(), 1);
        let mut text = Reply::passed();
        let plain = Answer {
            body: "not json".into(),
            ..answer
        };
        register_answer(&mut text, &api, &plain);
        assert!(text.scan.is_empty());
    }

    #[test]
    fn a_refused_connection_names_the_method_and_the_url_without_its_query() {
        let api = api(&json!({}));
        let (reply, exchange) = send("unit-refused", &recipe(), &api, true);
        let v: Value = serde_json::from_str(&reply.json()).expect("JSON");
        assert_eq!(v["status"], "fatal");
        assert_eq!(v["class"], "transport");
        assert!(v["diagnostics"].as_array().is_some_and(Vec::is_empty));
        let error = v["error"].as_str().expect("error");
        assert!(
            error.starts_with("POST http://127.0.0.1:1/p: request failed"),
            "{error}"
        );
        assert!(
            !error.contains("api_token") && !error.contains("q=1"),
            "{error}"
        );
        assert!(exchange.is_none());
        // The credentials were registered although nothing came back.
        assert_eq!(v["secrets"], json!(["Bearer abc1234", "abc1234"]));
    }
}
