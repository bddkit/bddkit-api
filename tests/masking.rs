//! What the plugin tells the host to hide, seen through the real `bddkit`: the
//! plugin sends real values and registers what is sensitive, and the host's
//! reports, console and stream must never show them. Ported from the core's
//! acceptance suite with their meaning unchanged, except where a test ended on
//! a response assertion (a later issue ports those and the failure dump).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{api_project, evidence, run_with_junit, scenario};
use serde_json::json;

/// A step that fails and quotes `note`, so only registration can hide it.
const QUOTE: &str = "    Then variable \"note\" should be equal to \"nothing\"\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_json_value_sent_under_a_sensitive_name_is_secret_everywhere_afterwards() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: sent, not echoed
    Given the request body is:
      """
      {"token": "tok-json-value-1"}
      """
    When I request "/ping" using HTTP POST
    And set variable "note" to "tok-json-value-1"
    Then variable "note" should be equal to "nothing"
"#;
    let cfg = api_project("mask-json-request", &base, feature, "", "");
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(!xml.contains("tok-json-value-1"), "{xml}");
    assert!(!stdout.contains("tok-json-value-1"), "{stdout}");
}

/// The token exists only in the answer: nothing sent registered it and the
/// cookie header that also carries it is off the list, so only the scan of the
/// response body can hide it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_only_the_response_carries_is_masked_by_the_response_scan() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: login
    Given the request body is:
      """
      {"user": "ann", "password": "correct"}
      """
    When I request "/login" using HTTP POST
    And set variable "note" to "tok-abc"
    Then variable "note" should be equal to "nothing"
"#;
    let cfg = api_project(
        "mask-response-only",
        &base,
        feature,
        "      sensitive_headers_only: []\n",
        "privacy:\n  fields: [jwt]\n",
    );
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(!xml.contains("tok-abc"), "{xml}");
    assert!(!stdout.contains("tok-abc"), "{stdout}");
}

/// A response header under a sensitive name is registered whole and split, on
/// any status and with the evidence off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cookie_only_the_response_sets_is_masked_afterwards() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = format!(
        "{}{}",
        scenario(
            "    Given the request body is:\n      \"\"\"\n      {\"user\": \"ann\", \"password\": \"correct\"}\n      \"\"\"\n    When I request \"/login\" using HTTP POST\n    And set variable \"note\" to \"tok-abc\"\n"
        ),
        QUOTE
    );
    let cfg = api_project("mask-set-cookie", &base, &feature, "", "");
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(
        !xml.contains("tok-abc") && !stdout.contains("tok-abc"),
        "{xml}{stdout}"
    );
}

/// The value is written in the path, and nothing else names it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_literal_query_value_in_the_path_joins_the_set() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: token in the path
    When I request "/ping?api_token=path-token-value-1"
    And set variable "note" to "path-token-value-1"
    Then variable "note" should be equal to "nothing"
"#;
    let cfg = api_project("mask-path-query", &base, feature, "", "");
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(!xml.contains("path-token-value-1"), "{xml}");
    assert!(!stdout.contains("path-token-value-1"), "{stdout}");
}

/// A value holding `&`, `=`, `+` or `%XX` is registered whole, not split.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_query_value_with_reserved_characters_is_registered_whole() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: reserved characters
    Given the query parameter "api_token" is "ab&cd=ef+gh%41-1234"
    When I request "/ping"
    And set variable "note" to "ab&cd=ef+gh%41-1234"
    Then variable "note" should be equal to "nothing"
"#;
    let cfg = api_project("mask-reserved", &base, feature, "", "");
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(
        !xml.contains("cd=ef+gh") && !stdout.contains("cd=ef+gh"),
        "{xml}{stdout}"
    );
}

/// Nothing is ever sent, so only registering at the step can hide the header.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_header_set_by_a_step_is_secret_even_if_nothing_is_sent() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: header, no request
    Given the "Authorization" request header is "Bearer unsent-token-value-1"
    And the "X-Trace" request header is "plain-trace-value"
    And set variable "note" to "Bearer unsent-token-value-1"
    Then variable "note" should be equal to "plain-trace-value"
"#;
    let cfg = api_project("mask-unsent", &base, feature, "", "");
    let (code, stdout, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    assert!(!xml.contains("unsent-token-value-1"), "{xml}");
    assert!(xml.contains("plain-trace-value"), "{xml}");
}

const WITH_AUTH: &str = r#"Feature: masking
  Scenario: a request
    Given the "Authorization" request header is "Bearer s3cr3t-token-value"
    And the "X-Api-Key" request header is "key-value-12345"
    When I request "/ping"
"#;

/// The request never gets an answer, so only what was registered can hide the
/// token that an earlier step's text carries into the report.
#[test]
fn a_header_is_masked_in_the_report_even_when_the_request_never_connects() {
    let bin = require_bddkit!();
    let cfg = api_project("mask-refused", common::CLOSED, WITH_AUTH, "", "");
    let (code, _, xml) = run_with_junit(&bin, &cfg);
    assert_eq!(code, 1);
    assert!(xml.contains("request failed"), "{xml}");
    assert!(xml.contains("GET http://127.0.0.1:1/"), "{xml}");
    assert!(!xml.contains("s3cr3t-token-value"), "{xml}");
}

fn request_headers(events: &[serde_json::Value]) -> Vec<serde_json::Value> {
    events[0]["content"]["request"]["headers"]
        .as_array()
        .expect("headers")
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sensitive_headers_extend_the_default_list() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project(
        "mask-hdr-extend",
        &base,
        WITH_AUTH,
        "      sensitive_headers: [x-api-key]\n",
        "",
    );
    let events = evidence(&bin, &cfg);
    let headers = request_headers(&events);
    assert!(
        headers.contains(&json!(["Authorization", "***"])),
        "{headers:?}"
    );
    assert!(
        headers.contains(&json!(["X-Api-Key", "***"])),
        "{headers:?}"
    );
    assert!(
        !events[0].to_string().contains("key-value-12345"),
        "{events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sensitive_headers_only_replaces_the_default_list() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project(
        "mask-hdr-only",
        &base,
        WITH_AUTH,
        "      sensitive_headers_only: [x-api-key]\n",
        "",
    );
    let events = evidence(&bin, &cfg);
    let headers = request_headers(&events);
    assert!(
        headers.contains(&json!(["X-Api-Key", "***"])),
        "{headers:?}"
    );
    assert!(
        headers.contains(&json!(["Authorization", "Bearer s3cr3t-token-value"])),
        "{headers:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_masked_header_keeps_its_name_in_the_evidence() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the \"Authorization\" request header is \"Bearer s3cret-credential\"\n    When I request \"/ping\"\n",
    );
    let cfg = api_project("evidence-header", &base, &feature, "", "");
    let events = evidence(&bin, &cfg);
    assert!(request_headers(&events).contains(&json!(["Authorization", "***"])));
    assert!(
        !events.iter().any(|e| e.to_string().contains("s3cret")),
        "{events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_field_is_masked_in_the_evidence_of_the_step_that_received_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    // `pw` is shorter than `min_length`: only its position hides it.
    let feature = scenario(
        "    Given the request body is:\n      \"\"\"\n      {\"user\": \"ann\", \"password\": \"pw\"}\n      \"\"\"\n    When I request \"/echo\" using HTTP POST\n",
    );
    let cfg = api_project("evidence-field", &base, &feature, "", "");
    let events = evidence(&bin, &cfg);
    let content = &events[0]["content"];
    for body in [&content["request"]["body"], &content["response"]["body"]] {
        let body = body.as_str().expect("an inline body");
        assert!(body.contains("ann"), "{body}");
        assert!(body.contains("***") && !body.contains("\"pw\""), "{body}");
    }
}

/// `--reveal` keeps its meaning: the value is shown, and the run says why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reveal_turns_masking_off_and_says_so() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project("mask-reveal", &base, WITH_AUTH, "", "");
    let events = cfg.with_file_name("events.ndjson");
    let (_, _, stderr) = common::run(
        &bin,
        &cfg,
        &[
            "--reveal",
            "--events",
            events.to_str().expect("UTF-8 path"),
            "--evidence",
        ],
    );
    assert!(stderr.contains("masking is off"), "{stderr}");
    let text = std::fs::read_to_string(&events).expect("stream");
    assert!(text.contains("Bearer s3cr3t-token-value"), "{text}");
}
