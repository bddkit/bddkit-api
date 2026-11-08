//! The steps that read the last response: `extract` and `Print response …`,
//! seen through the real `bddkit`. Ported from the core's acceptance suite
//! with their meaning unchanged.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{api_project, bddkit, combined, project, run, scenario};
use std::path::Path;

const LOGIN: &str = "    Given the request body is:\n      \"\"\"\n      {\"password\": \"correct\"}\n      \"\"\"\n    When I request \"/login\" using HTTP POST\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_printed_header_is_masked_and_reveal_shows_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(&format!("{LOGIN}    Then Print response headers\n"));
    let cfg = api_project("print-headers", &base, &feature, "", "");

    // No value is masked by value at this length: only the list of names can
    // hide the cookie, so a wrong list shows up.
    let (code, stdout, stderr) = run(&bin, &cfg, &["--privacy-min-length", "64"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stderr.contains("=== Response headers ==="), "{stderr}");
    assert!(stderr.contains("x-trace"), "{stderr}");
    assert!(!stderr.contains("tok-abc"), "{stderr}");
    assert!(shown_masked(&stderr, "set-cookie"), "{stderr}");

    let (code, stdout, stderr) = run(&bin, &cfg, &["--reveal"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stderr.contains("tok-abc"), "{stderr}");
}

/// The printed line of a header, hidden whole.
fn shown_masked(stderr: &str, header: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.contains(header) && line.contains("***"))
}

/// The masking list is the one of the API the response came from, even after
/// the scenario has moved to another API.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_printed_header_uses_the_masking_of_the_api_it_came_from() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(&format!(
        "{LOGIN}    When I use \"plain\" api\n    Then Print response headers\n"
    ));
    let resources = format!(
        "  api:\n    stub:\n      base_url: {base}\n    plain:\n      base_url: {base}\n      sensitive_headers_only: [authorization]\n"
    );
    let cfg = project(
        "print-headers-switch",
        &feature,
        &resources,
        "default_api: stub\n",
    );
    let (code, stdout, stderr) = run(&bin, &cfg, &["--privacy-min-length", "64"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stderr.contains("tok-abc"), "{stderr}");
    assert!(shown_masked(&stderr, "set-cookie"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_printed_json_body_is_masked_and_reveal_shows_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given the \"Content-Type\" request header is \"application/json\"\n    And the request body is:\n      \"\"\"\n      {\"token\": \"tok-json-value-1\"}\n      \"\"\"\n    When I request \"/echo\" using HTTP POST\n    Then Print response body\n    And Print response body as \"received\"\n",
    );
    let cfg = api_project("print-body", &base, &feature, "", "");

    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stderr.contains("\"received\""), "{stderr}");
    assert!(!stderr.contains("tok-json-value-1"), "{stderr}");

    let (_, _, stderr) = run(&bin, &cfg, &["--reveal"]);
    assert!(stderr.contains("tok-json-value-1"), "{stderr}");
}

// core: print_body_as_path_fails_for_plain_content_type and
// print_body_as_path_succeeds_on_real_world_html, one stub for both
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn printing_a_path_of_a_plain_body_fails_and_a_loose_html_one_prints() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let cfg = api_project(
        "print-plain",
        &base,
        &scenario("    When I request \"/plain\"\n    Then Print response body as \"x\"\n"),
        "",
        "",
    );
    let (code, stdout, _) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}");
    assert!(
        stdout.contains("path selection is not supported: content-type is not JSON/XML/HTML"),
        "{stdout}"
    );

    let cfg = api_project(
        "print-loose",
        &base,
        &scenario(
            "    When I request \"/html-loose\"\n    Then Print response body as \"div.box\"\n",
        ),
        "",
        "",
    );
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stderr.contains("Loose"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_global_extract_warns_when_a_file_level_variable_shadows_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = scenario(
        "    Given set variable \"seen\" to \"file\"\n    When I request \"/ping\"\n    Then extract \"version\" from JSON as \"seen\" global\n    And variable \"seen\" should be equal to \"file\"\n    And extract \"version\" from JSON as \"fresh\" global\n    And variable \"fresh\" should be equal to \"3\"\n",
    );
    let cfg = api_project("extract-shadow", &base, &feature, "", "");
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("shadows it"), "{stdout}");
    assert_eq!(stdout.matches("shadows it").count(), 1, "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_global_extract_survives_the_scenario() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = "Feature: f\n  Scenario: a\n    When I request \"/ping\"\n    Then extract \"version\" from JSON as \"kept\" global\n    And extract \"version\" from JSON as \"local\"\n  Scenario: b\n    Then variable \"kept\" should be equal to \"3\"\n    And variable \"local\" should be equal to \"3\"\n";
    let cfg = api_project("extract-global", &base, feature, "", "");
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_extract_sets_nothing_and_names_its_class() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    for (steps, text) in [
        (
            "    Then extract \"a\" from JSON as \"x\"\n",
            "no request has been sent yet",
        ),
        (
            "    When I request \"/ping\"\n    Then extract \"nope\" from JSON as \"x\"\n",
            "nope",
        ),
        (
            "    When I request \"/ping\"\n    Then extract \"jwt_token\" from cookies as \"x\"\n",
            "cookie \"jwt_token\" not found in the response",
        ),
        (
            "    When I request \"/html\"\n    Then extract \".missing\" from response body as \"x\"\n",
            "matched no elements",
        ),
        (
            "    When I request \"/ping\"\n    Then extract \"p\" from response body as \"x\"\n",
            "element selectors need HTML or XML content",
        ),
    ] {
        let cfg = api_project("extract-fail", &base, &scenario(steps), "", "");
        let (code, stdout, _) = run(&bin, &cfg, &[]);
        assert_eq!(code, 1, "{steps}{stdout}");
        assert!(stdout.contains(text), "{steps}{stdout}");
    }
}

fn doctor_pair(name: &str, caller: &str, helper: &str) -> (Option<i32>, String) {
    let bin = common::bddkit_bin().expect("BDDKIT_BIN");
    let cfg = api_project(name, "http://127.0.0.1:1/", caller, "", "");
    let features = cfg.parent().expect("parent").join("features");
    std::fs::write(features.join("helper.feature"), helper).expect("helper");
    let out = bddkit(&bin, &["doctor", "--config", path(&cfg)]);
    (out.status.code(), combined(&out))
}

fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 path")
}

const HELPER: &str = "Feature: helper\n  Scenario: make\n    Given set variable \"x\" to \"1\"\n";

#[test]
fn doctor_hints_at_an_extract_after_an_include_but_not_at_an_assertion() {
    let _ = require_bddkit!();
    let caller = "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\"\n    And extract \"id\" from JSON as \"team_id\"\n    And the response code is 200\n";
    let (code, text) = doctor_pair("doctor-extract", caller, HELPER);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("only.feature:4"), "{text}");
    assert!(
        text.contains("reads the response the include left behind"),
        "{text}"
    );
    assert!(!text.contains("only.feature:5"), "{text}");
}

#[test]
fn doctor_stays_quiet_about_an_export_an_extract_sets_and_names_one_nothing_sets() {
    let _ = require_bddkit!();
    let caller = "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\"\n";
    let (code, text) = doctor_pair(
        "doctor-export-ok",
        caller,
        "Feature: helper\n  @exports(team_id)\n  Scenario: make\n    Given extract \"id\" from JSON as \"team_id\"\n",
    );
    assert_eq!(code, Some(0), "{text}");
    assert!(!text.contains("team_id"), "{text}");

    let (code, text) = doctor_pair(
        "doctor-export-missing",
        caller,
        "Feature: helper\n  @exports(team_idd)\n  Scenario: make\n    Given extract \"id\" from JSON as \"team_id\"\n",
    );
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("team_idd"), "{text}");
}

#[test]
fn the_steps_are_listed_under_api() {
    let bin = require_bddkit!();
    let cfg = api_project("steps-list", common::CLOSED, common::PING, "", "");
    let out = bddkit(&bin, &["steps", "list", "api", "--config", path(&cfg)]);
    let text = combined(&out);
    for step in [
        "extract \"<path>\" from JSON as \"<name>\" global",
        "extract \"<cookie>\" from cookies as \"<name>\"",
        "extract \"<selector>\" from response body as \"<name>\"",
        "Print response headers",
        "Print response body as \"<path>\"",
    ] {
        assert!(text.contains(step), "{step}\n{text}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_global_cookie_extract_survives_the_scenario() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = format!(
        "Feature: f\n  Scenario: a\n{LOGIN}    Then extract \"jwt_token\" from cookies as \"kept\" global\n  Scenario: b\n    Then variable \"kept\" should be equal to \"tok-abc\"\n"
    );
    let cfg = api_project("extract-cookie-global", &base, &feature, "", "");
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
}
