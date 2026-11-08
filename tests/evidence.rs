//! The HTTP exchange as evidence, over the real `bddkit`: which step owns it,
//! where a long body goes, and what a held (deferred) exchange hides. Ported
//! from the core's acceptance suite with their meaning unchanged.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::events::{of_type, read_stream, run_env, run_with_events};
use common::{api_project, scenario};
use serde_json::Value;
use std::path::{Path, PathBuf};

const TIMEOUT: &str = "      timeout_secs: 2\n";

/// One scenario of `steps`, run with the evidence on; the stream it wrote.
fn http_evidence_run(
    name: &str,
    base: &str,
    bin: &Path,
    steps: &str,
    extra: &[&str],
) -> Vec<Value> {
    let cfg = api_project(name, base, &scenario(steps), TIMEOUT, "");
    let mut args = vec!["--evidence"];
    args.extend_from_slice(extra);
    run_with_events(bin, &cfg, &args).1
}

/// `steps` against the stub, run with `--events` and then `args`.
fn http_modes_run(name: &str, base: &str, bin: &Path, feature: &str, args: &[&str]) -> Vec<Value> {
    let cfg = api_project(name, base, feature, TIMEOUT, "");
    run_with_events(bin, &cfg, args).1
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_exchange_is_evidence_of_the_step_that_sent_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let stream = http_evidence_run(
        "evidence-http",
        &base,
        &bin,
        "    Given set variable \"a\" to \"1\"\n    When I request \"/ping\"\n    Then the response code is 200\n",
        &[],
    );

    let evidence = of_type(&stream, "evidence");
    assert_eq!(evidence.len(), 1, "{stream:?}");
    let e = evidence[0];
    assert_eq!(e["step"], 1, "the request, not the assertion after it");
    assert_eq!(
        (
            e["group"].as_str(),
            e["resource"].as_str(),
            e["kind"].as_str()
        ),
        (Some("api"), Some("stub"), Some("http"))
    );
    assert_eq!(e["content"]["method"], "GET");
    assert_eq!(e["content"]["status"], 200);
    assert!(
        e["content"]["url"]
            .as_str()
            .expect("url")
            .ends_with("/ping")
    );
    assert!(e["content"]["duration_us"].is_u64());
    assert!(e["content"]["response"]["body"].is_string());
    // In the timeline: after the step starts, before it finishes.
    let at = |kind: &str, step: u64| {
        stream
            .iter()
            .position(|x| x["type"] == kind && x["step"] == step)
            .expect("event")
    };
    let position = stream.iter().position(|x| x == e).expect("evidence");
    assert!(at("step_started", 1) < position && position < at("step_finished", 1));
}

/// A POST of `size` bytes to the stub, which echoes it, with the artifact root
/// inside the project. The evidence event of the request, and the root.
fn long_body_run(name: &str, base: &str, bin: &Path, size: usize) -> (Value, PathBuf) {
    let feature = format!(
        "Feature: e\n  Scenario: s\n    Given the request body is:\n      \"\"\"\n      {{\"note\": \"{}\"}}\n      \"\"\"\n    When I request \"/echo\" using HTTP POST\n",
        "x".repeat(size)
    );
    let cfg = api_project(name, base, &feature, TIMEOUT, "");
    let root = cfg.with_file_name("evidence");
    let (_, stream) = run_with_events(
        bin,
        &cfg,
        &[
            "--evidence",
            "--artifacts-dir",
            root.to_str().expect("UTF-8"),
        ],
    );
    (of_type(&stream, "evidence")[0].clone(), root)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_over_the_limit_is_written_to_a_file_and_referenced() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let (event, root) = long_body_run("evidence-long", &base, &bin, 20_000);

    let request = &event["content"]["request"]["body"];
    let path = request["path"].as_str().expect("a file reference");
    assert!(!path.starts_with('/') && !path.contains('\\'), "{path}");
    assert_eq!(request["media_type"], "application/json");
    let written = std::fs::read_to_string(root.join(path)).expect("the file exists");
    assert_eq!(request["size"], written.len());
    assert!(written.len() > 16 * 1024 && written.contains("xxxx"));
    // Per body: the response is long too and is its own file.
    let response = &event["content"]["response"]["body"];
    assert_ne!(response["path"], request["path"]);

    let (event, _) = long_body_run("evidence-short", &base, &bin, 100);
    assert!(
        event["content"]["request"]["body"].is_string(),
        "a short body stays in the event"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_referenced_file_is_complete_when_its_event_is_read() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let (event, root) = long_body_run("evidence-complete", &base, &bin, 40_000);
    for side in ["request", "response"] {
        let body = &event["content"][side]["body"];
        let on_disk = std::fs::metadata(root.join(body["path"].as_str().expect("path")))
            .expect("exists")
            .len();
        assert_eq!(body["size"], on_disk, "{side}");
    }
}

/// A scenario that POSTs a body over the inline limit, then passes or fails.
fn http_long_deferred_run(
    name: &str,
    base: &str,
    bin: &Path,
    fail: bool,
    env: &[(&str, &str)],
) -> PathBuf {
    let feature = format!(
        "Feature: e\n  Scenario: s\n    Given the request body is:\n      \"\"\"\n      {{\"note\": \"{}\"}}\n      \"\"\"\n    When I request \"/echo\" using HTTP POST\n    Then the response code is {}\n",
        "x".repeat(20_000),
        if fail { 500 } else { 201 }
    );
    let cfg = api_project(name, base, &feature, TIMEOUT, "");
    let root = cfg.with_file_name("evidence");
    let events = cfg.with_file_name("events.ndjson");
    let (code, _, _) = run_env(
        bin,
        &cfg,
        &[
            "--events",
            events.to_str().expect("UTF-8"),
            "--evidence=failures",
            "--artifacts-dir",
            root.to_str().expect("UTF-8"),
        ],
        env,
    );
    assert_eq!(code, i32::from(fail));
    root
}

fn files_under(root: &Path) -> usize {
    std::fs::read_dir(root).map_or(0, |dirs| {
        dirs.flatten()
            .map(|dir| std::fs::read_dir(dir.path()).map_or(0, Iterator::count))
            .sum()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_body_of_a_passing_scenario_never_reaches_the_disk_under_the_buffer_cap() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let root = http_long_deferred_run("deferred-long-pass", &base, &bin, false, &[]);
    assert_eq!(files_under(&root), 0, "held in memory, then dropped");

    let root = http_long_deferred_run("deferred-long-fail", &base, &bin, true, &[]);
    assert_eq!(
        files_under(&root),
        2,
        "request and response, once it failed"
    );
    let stream = read_stream(&root.with_file_name("events.ndjson"));
    let body = &of_type(&stream, "evidence")[0]["content"]["response"]["body"];
    let on_disk = std::fs::metadata(root.join(body["path"].as_str().expect("path")))
        .expect("exists")
        .len();
    assert_eq!(body["size"], on_disk);

    let root = http_long_deferred_run(
        "deferred-long-cap",
        &base,
        &bin,
        false,
        &[("BDDKIT_EVIDENCE_BUFFER", "0")],
    );
    assert_eq!(files_under(&root), 2, "past the cap it is written at once");
}

/// Held evidence is emitted at the end of the scenario, so a value that
/// became a secret after the step that sent it is masked there too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_evidence_is_masked_with_the_secrets_known_when_it_is_written() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let stream = http_modes_run(
        "deferred-late-secret",
        &base,
        &bin,
        "Feature: e\n  Scenario: s\n    Given the \"X-Note\" request header is \"late-secret-value-1\"\n    When I request \"/ping\"\n    And set variable \"my_secret\" to \"late-secret-value-1\"\n    Then the response code is 500\n",
        &["--evidence=failures"],
    );
    let evidence = of_type(&stream, "evidence");
    assert_eq!(evidence.len(), 1, "{stream:?}");
    let text = evidence[0].to_string();
    assert!(!text.contains("late-secret-value-1"), "{text}");
    assert!(text.contains("***"), "{text}");
}
