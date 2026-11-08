//! Polling against the real host: an eventual assertion replays the saved
//! request (method and body included), waits for an absence, and reports the
//! last mismatch with the final exchange when it runs out. Each test starts
//! its own small service, because the answer has to change between attempts.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::{get, post};
use axum::{Json, Router};
use common::{api_project, run};
use serde_json::{Value, json};

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let address = listener.local_addr().expect("stub address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve stub");
    });
    format!("http://{address}/")
}

async fn spawn_eventual_post_stub(ready_on: Option<usize>) -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = Arc::clone(&calls);
    let app = Router::new().route(
        "/eventual",
        post(move |body: String| {
            let handler_calls = Arc::clone(&handler_calls);
            async move {
                let call = handler_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let state = if ready_on.is_some_and(|ready| call >= ready) {
                    "ready"
                } else {
                    "pending"
                };
                let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                Json(json!({"state": state, "payload": payload}))
            }
        }),
    );
    (serve(app).await, calls)
}

fn write_eventual_post_project(base: &str, name: &str) -> std::path::PathBuf {
    api_project(
        name,
        base,
        r#"Feature: eventual POST response
  Scenario: replay the saved request until the response is ready
    Given the request body is:
      """
      {"request":"saved"}
      """
    When I request "/eventual" using HTTP POST
    And I expect the next assertion to pass within "1" seconds, checking every "25" milliseconds
    Then the response body equals JSON:
      """
      {"state":"ready","payload":{"request":"saved"}}
      """
"#,
        "",
        "",
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eventual_post_response_replays_the_saved_method_and_body() {
    let bin = require_bddkit!();
    let (base, calls) = spawn_eventual_post_stub(Some(2)).await;
    let config = write_eventual_post_project(&base, "eventual-post-success");

    let (code, stdout, stderr) = run(&bin, &config, &[]);

    assert_eq!(
        code, 0,
        "eventual POST must pass after one replay\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

async fn spawn_eventual_absence_stub(ready_on: usize) -> String {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/eventual-absence",
        post(move |_body: String| {
            let calls = Arc::clone(&calls);
            async move {
                let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call >= ready_on {
                    Json(json!({"state": "ready"}))
                } else {
                    Json(json!({"state": "pending", "pendingReason": "not ready yet"}))
                }
            }
        }),
    );
    serve(app).await
}

fn write_eventual_absence_project(base: &str, name: &str) -> std::path::PathBuf {
    api_project(
        name,
        base,
        r#"Feature: eventual absence
  Scenario: poll until a node disappears
    When I request "/eventual-absence" using HTTP POST
    And I expect the next assertion to pass within "1" seconds, checking every "25" milliseconds
    Then the JSON node "pendingReason" should not exist
"#,
        "",
        "",
    )
}

/// Issue #22, item 7: "wait for absence" must be explicitly validated, not just
/// assumed to work by composition with the existing polling mechanism.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eventual_absence_polls_until_the_node_disappears() {
    let bin = require_bddkit!();
    let base = spawn_eventual_absence_stub(2).await;
    let config = write_eventual_absence_project(&base, "eventual-absence-success");

    let (code, stdout, stderr) = run(&bin, &config, &[]);

    assert_eq!(
        code, 0,
        "polling for absence must pass once the node disappears\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eventual_post_timeout_reports_last_mismatch_and_final_exchange() {
    let bin = require_bddkit!();
    let (base, _calls) = spawn_eventual_post_stub(None).await;
    let config = write_eventual_post_project(&base, "eventual-post-timeout");

    let (code, stdout, _) = run(&bin, &config, &[]);

    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("did not pass within 1s"), "{stdout}");
    assert!(
        stdout.contains("root.state")
            && stdout.contains("expected: \"ready\"")
            && stdout.contains("actual:   \"pending\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("POST http://")
            && stdout.contains("/eventual")
            && stdout.contains("← 200")
            && stdout.contains(r#"{"request":"saved"}"#)
            && stdout.contains(r#""state":"pending""#),
        "{stdout}"
    );
}

/// The value a `<<unique(number)>>` in the polled step is prepared to must not
/// change between attempts, or the assertion could never hold. Seen from
/// outside: the counter moves by two between a value taken before the
/// assertion and one taken after it (the assertion's own value in between),
/// however many attempts it made.
// core: the unit test of the same name in src/runner.rs, which read the counter in-process.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retrying_assertion_prepares_unique_capture_once() {
    let bin = require_bddkit!();
    let base = serve(Router::new().route("/state", get(|| async { "pending" }))).await;
    let feature = "Feature: eventual assertion\n  Scenario: before\n    Given set variable \"before\" to \"<<unique(number)>>\"\n    Then variable \"before\" should be equal to \"-\"\n  Scenario: stable preparation\n    When I request \"/state\"\n    And I expect the next assertion to pass within \"1\" seconds, checking every \"100\" milliseconds\n    Then the response body contains \"<<unique(number)>>\"\n  Scenario: after\n    Given set variable \"after\" to \"<<unique(number)>>\"\n    Then variable \"after\" should be equal to \"-\"\n";
    let config = api_project("eventual-unique", &base, feature, "", "");

    let (code, stdout, _) = run(&bin, &config, &[]);

    assert_eq!(code, 1, "{stdout}");
    let number = |pattern: &str| -> u64 {
        regex::Regex::new(pattern)
            .unwrap()
            .captures(&stdout)
            .and_then(|caps| caps[1].parse().ok())
            .unwrap_or_else(|| panic!("no {pattern} in {stdout}"))
    };
    let attempts = number(r"(\d+) attempts");
    assert!(
        attempts > 1,
        "the assertion was never retried: {attempts}\n{stdout}"
    );
    let actuals: Vec<u64> = regex::Regex::new(r#"actual:\s+"?(\d+)"#)
        .unwrap()
        .captures_iter(&stdout)
        .map(|caps| caps[1].parse().unwrap())
        .collect();
    assert_eq!(actuals.len(), 2, "{stdout}");
    assert_eq!(
        actuals[1] - actuals[0],
        2,
        "unique() must be evaluated only once\n{stdout}"
    );
}
