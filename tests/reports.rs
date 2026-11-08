//! Reports and the HTTP exchange, over the real `bddkit`. Ported from the
//! core's acceptance suite with their meaning unchanged.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use axum::routing::post;
use axum::{Json, Router};
use common::{api_project, bddkit, combined, run, run_with_junit};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The failed request's exchange is part of the failure in the JUnit report.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_junit_failure_holds_the_http_exchange() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: reported feature
  Scenario: passes
    When I request "/ping"
    Then the response code is 200

  Scenario: fails
    When I request "/ping"
    And set variable "x" to "<<null>>"
    Then variable "x" should be equal to "]]>"
    And the response code is 200
"#;
    let config = api_project("junit-http", &base, feature, "", "");
    let (code, _, xml) = run_with_junit(&bin, &config);
    assert_eq!(code, 1);
    assert!(xml.contains("GET http://"), "{xml}");
}

/// A POST stub that counts its calls and never reaches "ready".
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind eventual stub");
    let address = listener.local_addr().expect("eventual stub address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve eventual stub");
    });
    (format!("http://{address}/"), calls)
}

const EVENTUAL: &str = r#"Feature: eventual POST response
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
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unwritable_report_path_is_a_startup_failure_and_doctor_reports_it() {
    let bin = require_bddkit!();
    let (base, calls) = spawn_eventual_post_stub(Some(1)).await;
    let config = api_project("unwritable-report", &base, EVENTUAL, "", "");
    // A path under a regular file cannot be created.
    let report = config.join("cucumber.json");
    let report = report.to_str().expect("UTF-8 path");

    let (code, _, stderr) = run(&bin, &config, &["--cucumber-json", report]);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("cucumber.json"),
        "the path is named: {stderr}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no request may leave before the report is prepared"
    );

    let out = bddkit(
        &bin,
        &[
            "doctor",
            "--config",
            config.to_str().expect("UTF-8 path"),
            "--cucumber-json",
            report,
        ],
    );
    let stdout = combined(&out);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("✗ reports"), "{stdout}");
}
