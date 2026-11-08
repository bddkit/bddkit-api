//! What a request is made of when it leaves, read off the evidence: the steps
//! that build it, the one that sends it, and the exchange that comes back.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{api_project, evidence, run, scenario};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_get_carries_its_headers_and_query_and_comes_back_whole() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the \"Accept\" request header is \"text/plain\"\n    And I add \"application/json\" to the \"Accept\" request header\n    And the query parameter \"q\" is \"a b\"\n    When I request \"/headers?x=1\"\n",
    );
    let cfg = api_project("req-get", &base, &feature, "", "");
    let events = evidence(&bin, &cfg);
    assert_eq!(events.len(), 1, "{events:?}");
    let content = &events[0]["content"];
    assert_eq!(events[0]["kind"], "http");
    assert_eq!(events[0]["resource"], "stub");
    assert_eq!(content["method"], "GET");
    assert!(
        content["url"]
            .as_str()
            .expect("url")
            .ends_with("/headers?x=1&q=a+b")
    );
    assert_eq!(content["status"], 200);
    assert_eq!(
        content["request"]["headers"],
        json!([["Accept", "text/plain"], ["Accept", "application/json"]])
    );
    let response: serde_json::Value =
        serde_json::from_str(content["response"]["body"].as_str().expect("body")).expect("JSON");
    assert!(
        response["accept"].as_array().is_some_and(|a| a.len() == 2),
        "{response}"
    );
}

/// Read back from the wire by the stub, not from the plugin's own record.
fn wire(event: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(event["content"]["response"]["body"].as_str().expect("body"))
        .expect("JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_form_travels_urlencoded_and_a_raw_body_travels_as_written() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the request form parameters are:\n      | name | value |\n      | a    | 1     |\n      | b    | x y   |\n    When I request \"/wire\" using HTTP POST\n    And the request body is:\n      \"\"\"\n      {\"k\": \"v\"}\n      \"\"\"\n    And I request \"/wire\" using HTTP POST\n    And the request body is empty\n    And I request \"/wire\" using HTTP POST\n",
    );
    let cfg = api_project("req-form", &base, &feature, "", "");
    let events = evidence(&bin, &cfg);
    assert_eq!(events.len(), 3, "{events:?}");
    let form = wire(&events[0]);
    assert_eq!(form["body"], "a=1&b=x+y");
    assert_eq!(form["content_type"], "application/x-www-form-urlencoded");
    let raw = wire(&events[1]);
    assert!(
        raw["body"]
            .as_str()
            .expect("body")
            .contains(r#"{"k": "v"}"#),
        "{raw}"
    );
    assert_ne!(raw["content_type"], "application/x-www-form-urlencoded");
    let empty = wire(&events[2]);
    assert_eq!(empty["body"], "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_form_table_without_the_name_value_header_is_an_input_failure() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the request form parameters are:\n      | key | val |\n      | a   | 1   |\n",
    );
    let cfg = api_project("req-bad-form", &base, &feature, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(
        stdout.contains("table must have exactly two columns: name | value"),
        "{stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_header_name_fails_the_send_as_an_input_failure() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the \"bad name\" request header is \"v\"\n    When I request \"/ping\"\n",
    );
    let cfg = api_project("req-bad-header", &base, &feature, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("failed to build HTTP request"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_headers_start_every_scenario() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = "Feature: f\n  Scenario: one\n    Given the \"X-Extra\" request header is \"1\"\n    When I request \"/ping\"\n  Scenario: two\n    When I request \"/ping\"\n";
    let cfg = api_project(
        "req-defaults",
        &base,
        feature,
        "      default_headers:\n        X-Default: d\n",
        "",
    );
    let events = evidence(&bin, &cfg);
    assert_eq!(events.len(), 2, "{events:?}");
    let names = |i: usize| -> Vec<String> {
        events[i]["content"]["request"]["headers"]
            .as_array()
            .expect("headers")
            .iter()
            .map(|p| p[0].as_str().expect("name").to_string())
            .collect()
    };
    assert_eq!(names(0), ["X-Default", "X-Extra"]);
    assert_eq!(
        names(1),
        ["X-Default"],
        "a scenario does not inherit the last one's request"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_passed_send_without_evidence_attaches_no_diagnostic() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project(
        "req-no-evidence",
        &base,
        &scenario("    When I request \"/ping\"\n"),
        "",
        "",
    );
    let events = cfg.with_file_name("events.ndjson");
    let (code, _, _) = run(
        &bin,
        &cfg,
        &["--events", events.to_str().expect("UTF-8 path")],
    );
    assert_eq!(code, 0);
    let text = std::fs::read_to_string(&events).expect("stream");
    assert!(!text.contains("\"type\":\"evidence\""), "{text}");
}
