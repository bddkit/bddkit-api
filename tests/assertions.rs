//! The response assertions, over the real host: the feature files the host's
//! own suite ran before the steps moved here. `response_body.feature` leaves
//! out the `extract` lines, which return with the `extract` steps.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::post;
use axum::{Json, Router};
use common::{api_project, project, scenario};
use serde_json::{Value, json};

/// `/eventual` answers `pending` until the `ready_on`-th call. Returns the
/// base URL and the call counter.
async fn eventual(ready_on: usize) -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let app = Router::new().route(
        "/eventual",
        post(move |body: String| {
            let call = seen.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                let state = if call >= ready_on { "ready" } else { "pending" };
                let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                Json(json!({"state": state, "payload": payload}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    (format!("http://{address}/"), calls)
}

/// `/poll` answers `pending` with a token that differs on every call.
async fn leaking() -> String {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/poll",
        post(move || {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            async move { Json(json!({"state": "pending", "token": format!("tok{call}-zq81xk92")})) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    format!("http://{address}/")
}

/// A service that answers its first request and is gone for the next.
async fn answers_once() -> String {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let stop = Arc::new(std::sync::Mutex::new(Some(stop)));
    let app = Router::new().route(
        "/once",
        axum::routing::get(move || {
            if let Some(stop) = stop.lock().expect("lock").take() {
                let _ = stop.send(());
            }
            async { "here" }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .expect("serve");
    });
    format!("http://{address}/")
}

const TWO_APIS: &str = "  api:\n    primary:\n      base_url: {primary}\n    secondary:\n      base_url: {secondary}\n";

fn two_apis(
    name: &str,
    features: &[(&str, &str)],
    primary: &str,
    secondary: &str,
) -> std::path::PathBuf {
    let resources = TWO_APIS
        .replace("{primary}", primary)
        .replace("{secondary}", secondary);
    let cfg = project(name, features[0].1, &resources, "default_api: primary\n");
    std::fs::remove_file(cfg.with_file_name("features/only.feature")).expect("rm");
    for (file, text) in features {
        std::fs::write(cfg.with_file_name("features").join(file), text).expect("feature");
    }
    cfg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_moved_feature_files_pass() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    // Not `tests/api/acceptance.yaml`: its macros use steps that are not here yet.
    let features = common::root().join("tests/api/features");
    let config = common::api_project("moved-features", &base, "", "", "");
    std::fs::write(
        &config,
        format!(
            "paths: [{}]\nresources:\n  api:\n    stub:\n      base_url: {base}\n",
            features.display().to_string().replace('\\', "/")
        ),
    )
    .expect("config");
    let out = common::bddkit(&bin, &["run", "--config", config.to_str().expect("UTF-8")]);
    let text = common::combined(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("files: 5") && text.contains("failed: 0"),
        "{text}"
    );
}

const SWITCH: &str = "Feature: switching\n  Scenario: the previous response survives the switch\n    When I request \"/ping\"\n    Then the response body contains JSON:\n      \"\"\"\n      {\"version\": 3}\n      \"\"\"\n    When I use \"secondary\" api\n    Then the response body contains JSON:\n      \"\"\"\n      {\"version\": 3}\n      \"\"\"\n    When I request \"/ping\"\n    Then the response body contains JSON:\n      \"\"\"\n      {\"source\": \"secondary\"}\n      \"\"\"\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_scenario_can_call_two_different_apis() {
    let bin = require_bddkit!();
    let primary = common::stub::spawn().await;
    let secondary = common::stub::spawn_secondary().await;
    let cfg = two_apis(
        "two-apis",
        &[("only.feature", SWITCH)],
        &primary,
        &secondary,
    );
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}");
    assert!(
        stdout.contains("files: 1, scenarios: 1, failed: 0"),
        "{stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_api_switch_inside_an_include_stays_switched_after_it_returns() {
    let bin = require_bddkit!();
    let primary = common::stub::spawn().await;
    let secondary = common::stub::spawn_secondary().await;
    let included = "Feature: Switches API inside an include\n  Scenario: Switches to a secondary API\n    Given I use \"secondary\" api\n    When I request \"/ping\"\n    Then the response body contains JSON:\n      \"\"\"\n      {\"source\": \"secondary\"}\n      \"\"\"\n";
    let caller = "Feature: Caller\n  Scenario: the switch persists after the include returns\n    Given I include \"include/state.feature\"\n    When I request \"/ping\"\n    Then the response body contains JSON:\n      \"\"\"\n      {\"source\": \"secondary\"}\n      \"\"\"\n";
    let cfg = two_apis(
        "include-switch",
        &[("caller.feature", caller)],
        &primary,
        &secondary,
    );
    let dir = cfg.with_file_name("features").join("include");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("state.feature"), included).expect("feature");
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}");
    assert!(
        stdout.contains("files: 2, scenarios: 2, failed: 0"),
        "{stdout}"
    );
}

const POLLED: &str = "    Given the request body is:\n      \"\"\"\n      {\"request\": \"saved\"}\n      \"\"\"\n    When I request \"/eventual\" using HTTP POST\n    And I expect the next assertion to pass within 2 seconds, checking every 25 milliseconds\n    Then the response body equals JSON:\n      \"\"\"\n      {\"state\": \"ready\", \"payload\": {\"request\": \"saved\"}}\n      \"\"\"\n";

fn stream(cfg: &std::path::Path, bin: &std::path::Path, extra: &[&str]) -> Vec<Value> {
    let events = cfg.with_file_name("events.ndjson");
    let mut args = vec!["--events", events.to_str().expect("UTF-8 path")];
    args.extend_from_slice(extra);
    common::run(bin, cfg, &args);
    std::fs::read_to_string(&events)
        .expect("event stream")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_polling_replay_is_a_new_exchange_and_an_attempt() {
    let bin = require_bddkit!();
    let (base, calls) = eventual(2).await;
    let cfg = api_project("replay", &base, &scenario(POLLED), "", "");
    let stream = stream(&cfg, &bin, &["--evidence"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let kinds: Vec<_> = stream
        .iter()
        .filter(|e| e["type"] == "evidence" || e["type"] == "attempt")
        .map(|e| (e["type"].as_str().expect("type"), e["step"].as_u64()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("evidence", Some(1)),
            ("attempt", Some(3)),
            ("evidence", Some(3))
        ],
        "the send, the first mismatch, the replay that passed"
    );
    let attempt = stream
        .iter()
        .find(|e| e["type"] == "attempt")
        .expect("attempt");
    assert_eq!(attempt["n"], 1);
    assert!(
        attempt["message"]
            .as_str()
            .is_some_and(|m| m.contains("pending")),
        "{attempt}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replay_goes_to_the_api_the_request_went_to() {
    let bin = require_bddkit!();
    let (primary, calls) = eventual(2).await;
    let secondary = common::stub::spawn_secondary().await;
    let feature = scenario(&POLLED.replace(
        "    And I expect",
        "    And I use \"secondary\" api\n    And I expect",
    ));
    let cfg = two_apis(
        "replay-api",
        &[("only.feature", &feature)],
        &primary,
        &secondary,
    );
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "both sends reached the first API"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_that_stayed_down_ends_a_poll_as_transport() {
    let bin = require_bddkit!();
    let base = answers_once().await;
    let feature = scenario(
        "    When I request \"/once\"\n    And I expect the next assertion to pass within 1 seconds, checking every 50 milliseconds\n    Then the response code is 500\n",
    );
    let cfg = api_project("stayed-down", &base, &feature, "", "");
    let (code, stdout, junit) = common::run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(junit.contains("type=\"transport\""), "{junit}");
}

/// The lines of the dump that name the exchange.
fn shown(stdout: &str) -> usize {
    stdout.matches("/ping").count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_assertion_shows_the_exchange_once() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario("    When I request \"/ping\"\n    Then the response code is 404\n");
    let cfg = api_project("dump-once", &base, &feature, "", "");
    let (code, plain, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{plain}");
    assert_eq!(shown(&plain), 1, "{plain}");
    let events = cfg.with_file_name("e.ndjson");
    let (_, with, _) = common::run(
        &bin,
        &cfg,
        &["--events", events.to_str().expect("UTF-8"), "--evidence"],
    );
    assert_eq!(shown(&with), 1, "{with}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_replay_shows_its_own_exchange_once() {
    let bin = require_bddkit!();
    let (base, _) = eventual(100).await;
    let feature = scenario(&POLLED.replace("within 2", "within 1"));
    let cfg = api_project("dump-replay", &base, &feature, "", "");
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(stdout.matches("/eventual").count(), 1, "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_core_step_still_shows_the_last_exchange() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    When I request \"/ping\"\n    Then variable \"nope\" should be equal to \"x\"\n",
    );
    let cfg = api_project("dump-core", &base, &feature, "", "");
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(shown(&stdout), 1, "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_sent_is_an_input_failure() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project(
        "none-sent",
        &base,
        &scenario("    Then the response code is 200\n"),
        "",
        "",
    );
    let (code, stdout, junit) = common::run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("no request has been sent yet"), "{stdout}");
    assert!(junit.contains("type=\"input\""), "{junit}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replayed_response_is_masked_like_the_first_one() {
    let bin = require_bddkit!();
    let base = leaking().await;
    let feature = scenario(
        "    When I request \"/poll\" using HTTP POST\n    And I expect the next assertion to pass within 1 seconds, checking every 50 milliseconds\n    Then the JSON node \"state\" should be null\n",
    );
    let cfg = api_project("replay-mask", &base, &feature, "", "");
    let (code, stdout, _) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("***"), "{stdout}");
    assert!(!stdout.contains("zq81xk92"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_poll_uses_the_options_of_the_api_the_request_went_to() {
    let bin = require_bddkit!();
    let (primary, calls) = eventual(4).await;
    let secondary = common::stub::spawn_secondary().await;
    // At the secondary's interval the poll makes too few attempts to reach the
    // fourth request, so it passes only if the primary's interval governs it.
    let resources = format!(
        "  api:\n    primary:\n      base_url: {primary}\n      options:\n        polling:\n          timeout_secs: 5\n          interval_ms: 25\n    secondary:\n      base_url: {secondary}\n      options:\n        polling:\n          timeout_secs: 1\n          interval_ms: 600\n"
    );
    let feature = scenario(&POLLED.replace(
        "    And I expect the next assertion to pass within 2 seconds, checking every 25 milliseconds",
        "    And I use \"secondary\" api\n    And I expect the next assertion to pass eventually",
    ));
    let cfg = project(
        "poll-options",
        &feature,
        &resources,
        "default_api: primary\n",
    );
    let (code, stdout, stderr) = common::run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
