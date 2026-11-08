//! `bddkit doctor` on a project with an `api` instance, ported from the
//! host's acceptance suite.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::ported::{closed_project, doctor, doctor_include_pair};
use common::{CLOSED, api_project, project};

const OFFLINE: &str = "Feature: only\n  Scenario: one\n    When I request \"/ping\"\n";

const TEAM_CALLER: &str = "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\"\n    And extract \"id\" from JSON as \"team_id\"\n";

#[test]
fn doctor_include_hints_follow_with_prefix_and_ignore_an_outline_placeholder() {
    let bin = require_bddkit!();
    // `with prefix "a"` renames the export, so the extract is still a hint.
    let (code, stdout) = doctor_include_pair(
        &bin,
        "doctor-include-prefix",
        "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\" with prefix \"a\"\n    And extract \"id\" from JSON as \"a_team_id\"\n",
        "Feature: helper\n  @exports(team_id)\n  Scenario: make\n    Given set variable \"team_id\" to \"1\"\n",
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(!stdout.contains("left behind"), "{stdout}");

    // A name made of an Outline column is not known before the row is chosen.
    let (code, stdout) = doctor_include_pair(
        &bin,
        "doctor-include-placeholder",
        "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\" with:\n      | k   |\n      | foo |\n",
        "Feature: helper\n  @exports(foo)\n  Scenario Outline: make\n    Given set variable \"<k>\" to \"1\"\n    Examples:\n      | k   |\n      | foo |\n",
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(!stdout.contains("sets it"), "{stdout}");
}

#[test]
fn doctor_hints_at_an_extract_that_reads_the_response_an_include_left_behind() {
    let bin = require_bddkit!();
    let (code, stdout) = doctor_include_pair(
        &bin,
        "doctor-include-extract",
        TEAM_CALLER,
        "Feature: helper\n  Scenario: make\n    Given set variable \"x\" to \"1\"\n",
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(stdout.contains("only.feature:4"), "{stdout}");
    assert!(stdout.contains("left behind"), "{stdout}");
}

#[test]
fn doctor_stays_quiet_about_a_helper_that_exports_what_it_sets() {
    let bin = require_bddkit!();
    let (code, stdout) = doctor_include_pair(
        &bin,
        "doctor-include-quiet",
        "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\"\n    Then variable \"team_id\" should be equal to \"1\"\n",
        "Feature: helper\n  @exports(team_id)\n  Scenario: make\n    Given extract \"id\" from JSON as \"team_id\"\n",
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(!stdout.contains("left behind"), "{stdout}");
    assert!(!stdout.contains("sets it"), "{stdout}");

    // The extract of an exported name is no hint either, and neither is
    // exporting a `with:` parameter, which the include seeds itself.
    let (code, stdout) = doctor_include_pair(
        &bin,
        "doctor-include-quiet-with",
        "Feature: only\n  Scenario: one\n    Given I include \"helper.feature\" with:\n      | tier  |\n      | basic |\n    And extract \"id\" from JSON as \"tier\"\n",
        "Feature: helper\n  @exports(tier)\n  Scenario Outline: make\n    Given set variable \"x\" to \"<tier>\"\n    Examples:\n      | tier |\n      | a    |\n",
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(!stdout.contains("left behind"), "{stdout}");
    assert!(!stdout.contains("sets it"), "{stdout}");
}

/// A step whose resource kind has nothing declared is a guaranteed failure in
/// the scenario, and `run` does not refuse the config — a `--tag` may keep the
/// scenario out. `doctor` applies no filter, so it is the one place to say so.
#[test]
fn doctor_reports_a_step_whose_resource_kind_declares_nothing() {
    let bin = require_bddkit!();
    let config = project(
        "doctor-unserved",
        "Feature: only\n  Scenario: one\n    When I request \"/ping\"\n    When I request \"/pong\"\n",
        "  api: {}\n",
        "",
    );
    let out = doctor(&bin, &config, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("only.feature:3"), "{stdout}");
    assert!(
        !stdout.contains("only.feature:4"),
        "once per file: {stdout}"
    );
    assert!(stdout.contains("resources.api declares none"), "{stdout}");
}

/// The promise the command is built on: `doctor` without `--live` must reach a
/// verdict on a train. The `base_url` here points at a closed port, and the
/// static run must still come back clean.
#[test]
fn doctor_without_live_leaves_an_unreachable_base_url_alone() {
    let bin = require_bddkit!();
    let config = closed_project("doctor-offline", OFFLINE, "");
    let out = doctor(&bin, &config, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a closed port is not a static problem\n{stdout}"
    );
    assert!(stdout.contains("live probe skipped"), "{stdout}");
}

/// A static run reports the probe of an API resource it did not make.
#[test]
fn doctor_json_reports_the_api_probe_a_static_run_did_not_make() {
    let bin = require_bddkit!();
    let config = api_project("doctor-json", CLOSED, OFFLINE, "", "");
    let out = doctor(&bin, &config, &["--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("--json emits JSON only");
    let checks = report["checks"].as_array().expect("checks is an array");
    assert!(
        checks
            .iter()
            .any(|c| c["stage"] == "api" && c["target"] == "stub" && c["status"] == "skipped"),
        "a static run reports the probe it did not make:\n{stdout}"
    );
}
