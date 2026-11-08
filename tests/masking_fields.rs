//! What the failure text shows, seen through the real `bddkit`: headers, config
//! secrets, `secret` parameters and the fields the host finds by the name the
//! system gave them. Ported from the core's acceptance suite with their
//! meaning unchanged.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::events::{of_type, read_stream, run_env};
use common::{api_project, run};

const FAILING_WITH_AUTH: &str = r#"Feature: masking
  Scenario: a failing request
    Given the "Authorization" request header is "Bearer s3cr3t-token-value"
    And the "X-Api-Key" request header is "key-value-12345"
    When I request "/ping"
    Then the response code is 500
"#;

fn variable_feature(set: &[(&str, &str)], check: &str) -> String {
    let mut text = String::from("Feature: masking\n  Scenario: a failing assertion\n");
    for (name, value) in set {
        text.push_str(&format!(
            "    Given set variable \"{name}\" to \"{value}\"\n"
        ));
    }
    text.push_str(&format!(
        "    Then variable \"{check}\" should be equal to \"nothing\"\n"
    ));
    text
}

/// A POST to /echo with `body`, whose answer the next step contradicts so the
/// dump is printed: the stub answers 201.
fn echo_feature(body: &str, extra_steps: &str) -> String {
    format!(
        "Feature: masking\n  Scenario: echo\n    Given the request body is:\n      \"\"\"\n      {body}\n      \"\"\"\n{extra_steps}    When I request \"/echo\" using HTTP POST\n    Then the response code is 200\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_authorization_header_is_masked_in_the_failure_dump() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project("mask-auth", &base, FAILING_WITH_AUTH, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("Authorization: ***"), "{stdout}");
    assert!(!stdout.contains("s3cr3t-token-value"), "{stdout}");
    // X-Api-Key is not on the default list.
    assert!(stdout.contains("key-value-12345"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_junit_report_carries_the_masked_failure_text() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project("mask-junit", &base, FAILING_WITH_AUTH, "", "");
    let (code, _, xml) = common::run_with_junit(&bin, &cfg);
    assert_eq!(code, 1);
    assert!(xml.contains("Authorization: ***"), "{xml}");
    assert!(!xml.contains("s3cr3t-token-value"), "{xml}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_config_key_with_a_field_word_is_masked_wherever_its_value_appears() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = variable_feature(&[("note", "cfg-secret-value-1")], "note");
    let cfg = api_project(
        "mask-config",
        &base,
        &feature,
        "      client_secret: cfg-secret-value-1\n",
        "",
    );
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(!stdout.contains("cfg-secret-value-1"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_from_an_env_reference_is_masked_like_one_written_in_the_file() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = variable_feature(&[("note", "env-secret-value-2")], "note");
    let cfg = api_project(
        "mask-config-env",
        &base,
        &feature,
        "      client_secret: ${MASK_TEST_SECRET}\n",
        "",
    );
    let (_, stdout, _) = run_env(
        &bin,
        &cfg,
        &[],
        &[("MASK_TEST_SECRET", "env-secret-value-2")],
    );
    assert!(!stdout.contains("env-secret-value-2"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn debug_output_on_stderr_is_masked() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: debug print
    Given set variable "secret_note" to "debug-value-1234"
    And I am in debug mode
    And the request body is:
      """
      {"note": "<<secret_note>>"}
      """
    When I request "/echo" using HTTP POST
    Then Print response body
"#;
    let cfg = api_project("mask-debug", &base, feature, "", "");
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("***"), "{stderr}");
    assert!(!stderr.contains("debug-value-1234"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slot_in_a_secret_parameter_stays_as_written_and_its_value_is_masked() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: a key from a variable
    Given set variable "hawk_key" to "hawk-slot-key-0001"
    And I sign the next request with Hawk id "session-1" and key "<<hawk_key>>"
    And the request body is:
      """
      {"code": "555555"}
      """
    When I request "/hawk" using HTTP POST
    Then the response code is 500
"#;
    let cfg = api_project("mask-slot", &base, feature, "", "");
    let (_, stdout, xml) = common::run_with_junit(&bin, &cfg);
    assert!(xml.contains("&lt;&lt;hawk_key&gt;&gt;"), "{xml}");
    assert!(!xml.contains("hawk-slot-key-0001") && !stdout.contains("hawk-slot-key-0001"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_in_a_login_response_is_masked_in_that_steps_own_failure_dump() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    // The stub echoes the request body back, as a login answer would carry its token.
    let feature = echo_feature(
        r#"{"user": "ann", "access_token": "tok-from-the-server-1"}"#,
        "",
    );
    let cfg = api_project("mask-token", &base, &feature, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(!stdout.contains("tok-from-the-server-1"), "{stdout}");
    assert!(stdout.contains(r#""access_token":"***""#), "{stdout}");
    assert!(stdout.contains("ann"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_field_is_found_at_any_depth_and_in_any_naming_style() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let body = r#"{"data": {"list": [{"refreshToken": "tok-camel-case-1"}, {"client-secret": "tok-kebab-case-2"}]}, "password": 12345678}"#;
    let cfg = api_project("mask-depth", &base, &echo_feature(body, ""), "", "");
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    for leaked in ["tok-camel-case-1", "tok-kebab-case-2", "12345678"] {
        assert!(!stdout.contains(leaked), "{leaked}: {stdout}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_field_that_only_contains_a_field_word_is_not_masked() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let body = r#"{"tokenizer": "plain-tokenizer-value", "key": "plain-key-value"}"#;
    let cfg = api_project("mask-contains", &base, &echo_feature(body, ""), "", "");
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(stdout.contains("plain-tokenizer-value"), "{stdout}");
    assert!(stdout.contains("plain-key-value"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_keeps_its_exact_text_around_a_masked_field() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    // Shorter than `min_length`: only the position can hide it.
    let body = "{ \"a\" :1,   \"token\"  :  \"Zq9\" ,\"z\":2 }";
    let cfg = api_project("mask-exact", &base, &echo_feature(body, ""), "", "");
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(
        stdout.contains("{ \"a\" :1,   \"token\"  :  \"***\" ,\"z\":2 }"),
        "{stdout}"
    );
    // The echoed answer is compact; its own field is hidden by position too.
    assert!(stdout.contains(r#""token":"***""#), "{stdout}");
    assert!(!stdout.contains("Zq9"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_form_field_and_query_parameter_are_masked() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: form and query
    Given the query parameter "api_token" is "query-token-value-1"
    And the query parameter "second_token" is "q1z"
    And the query parameter "page" is "2"
    And the request form parameters are:
      | name         | value               |
      | client_secret | form-secret-value-2 |
      | f_secret      | f2y                 |
      | name         | ann                 |
    When I request "/echo" using HTTP POST
    Then the response code is 200
"#;
    let cfg = api_project("mask-form", &base, feature, "", "");
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(!stdout.contains("query-token-value-1"), "{stdout}");
    assert!(!stdout.contains("form-secret-value-2"), "{stdout}");
    assert!(stdout.contains("api_token=***"), "{stdout}");
    // Too short to join the set: only their position can hide them.
    assert!(stdout.contains("second_token=***"), "{stdout}");
    assert!(stdout.contains("f_secret=***"), "{stdout}");
    assert!(
        !stdout.contains("q1z") && !stdout.contains("f2y"),
        "{stdout}"
    );
    assert!(stdout.contains("page=2"), "{stdout}");
    assert!(stdout.contains("client_secret=***"), "{stdout}");
    assert!(stdout.contains("name=ann"), "{stdout}");
}

/// A refused connection puts the whole URL into reqwest's message.
#[test]
fn a_query_secret_does_not_reach_the_error_of_a_refused_connection() {
    let bin = require_bddkit!();
    let feature = r#"Feature: masking
  Scenario: refused
    Given the query parameter "api_token" is "refused-token-value-1"
    And the query parameter "token" is "Zr7"
    When I request "/echo"
    Then the response code is 200
"#;
    let cfg = api_project("mask-refused-query", "http://127.0.0.1:1/", feature, "", "");
    let (code, stdout, xml) = common::run_with_junit(&bin, &cfg);
    assert_eq!(code, 1, "{stdout}");
    // The step that set the value passed and is listed in the report: only a
    // value registered before the request left can be hidden there.
    assert!(!xml.contains("refused-token-value-1"), "{xml}");
    assert!(stdout.contains("request failed"), "{stdout}");
    assert!(!stdout.contains("refused-token-value-1"), "{stdout}");
    assert!(!stdout.contains("Zr7"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_that_straddles_the_body_cut_is_masked_whole() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    // The secret starts 5 characters before the 600-character cut.
    let prefix = r#"{"pad": ""#;
    let pad = "x".repeat(595 - prefix.len() - r#"", "n": ""#.len());
    let secret = "straddle-secret-value-1";
    let body = format!(r#"{prefix}{pad}", "n": "{secret}"}}"#);
    let feature = echo_feature(
        &body,
        &format!("    And set variable \"secret_cut\" to \"{secret}\"\n"),
    );
    let cfg = api_project("mask-cut", &base, &feature, "", "");
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(stdout.contains(&pad), "the body is shown: {stdout}");
    assert!(
        !stdout.contains("stra"),
        "no first characters of it: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_fields_only_list_switches_the_field_rule_off() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let body = r#"{"access_token": "tok-visible-value-1"}"#;
    let cfg = api_project(
        "mask-fields-off",
        &base,
        &echo_feature(body, ""),
        "",
        "privacy:\n  fields_only: []\n",
    );
    let (_, stdout, _) = run(&bin, &cfg, &[]);
    assert!(stdout.contains("tok-visible-value-1"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn print_response_body_hides_a_short_sensitive_value_by_position() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: debug print
    Given the request body is:
      """
      {"note": "kept-visible", "token": "Zp4"}
      """
    When I request "/echo" using HTTP POST
    Then Print response body
"#;
    let cfg = api_project("mask-print-short", &base, feature, "", "");
    let (code, _, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("kept-visible"), "{stderr}");
    assert!(!stderr.contains("Zp4"), "{stderr}");
}

/// Decided (spec 7.5): a number under a sensitive name is hidden where it
/// stands and never joins the set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_number_under_a_sensitive_name_is_hidden_in_place_and_never_joins_the_set() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = r#"Feature: masking
  Scenario: numeric pin
    Given the request body is:
      """
      {"token": 87654321}
      """
    When I request "/echo" using HTTP POST
    And set variable "note" to "87654321"
    Then variable "note" should be equal to "nothing"
"#;
    let cfg = api_project("mask-number", &base, feature, "", "");
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains(r#""token":***"#), "{stdout}");
    // Not in the set: the plain variable still shows it.
    assert!(stdout.contains("actual:   87654321"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_in_a_description_is_masked_in_the_stream() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = "Feature: Secrets\n  Bearer cfg-secret-value-77 is in the feature description.\n  Scenario: first\n    Given set variable \"secret_note\" to \"stream-note-4321\"\n  Scenario: second\n    Quotes stream-note-4321 and Bearer cfg-secret-value-77 here.\n    Given set variable \"x\" to \"1\"\n";
    let cfg = api_project(
        "mask-description",
        &base,
        feature,
        "      default_headers:\n        Authorization: Bearer cfg-secret-value-77\n",
        "",
    );
    let events = cfg.with_file_name("events.ndjson");
    let (code, _, stderr) = run_env(
        &bin,
        &cfg,
        &["--events", events.to_str().expect("UTF-8 path")],
        &[],
    );
    assert_eq!(code, 0, "{stderr}");

    let text = std::fs::read_to_string(&events).expect("stream");
    assert!(!text.contains("cfg-secret-value-77"), "{text}");
    let stream = read_stream(&events);
    // Known from the config: hidden before the first scenario runs.
    assert_eq!(
        of_type(&stream, "file_started")[0]["description"],
        "*** is in the feature description."
    );
    // Learned in scenario one, quoted in scenario two: the file's set is shared.
    assert_eq!(
        of_type(&stream, "scenario_started")[1]["description"],
        "Quotes *** and *** here."
    );
}
