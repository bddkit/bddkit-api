//! Two feature files dispatching to the plugin at the same time. The stub's
//! `/barrier` answers 200 only when two requests are in flight together, so
//! the files passing proves the per-file handles and the shared clients do not
//! serialise, and the controls prove the barrier is not open for free.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use std::path::Path;

use common::barrier::spawn_barrier;

const FILE: &str = "Feature: f\n  Scenario: s\n    When I request \"/barrier\"\n    Then the response code is 200\n";

/// Two files, each one request to the barrier, under `concurrency` workers.
/// `tag` heads both files. Returns the exit code and the run's text.
async fn run_pair(bin: &Path, name: &str, concurrency: u32, tag: &str) -> (i32, String) {
    let base = spawn_barrier().await;
    let feature = format!("{tag}{FILE}");
    let config = common::api_project(
        name,
        &base,
        &feature,
        "",
        &format!("concurrency: {concurrency}\n"),
    );
    let features = config.with_file_name("features");
    std::fs::write(features.join("second.feature"), &feature).expect("second feature");
    let (code, stdout, stderr) = common::run(bin, &config, &[]);
    (code, format!("{stdout}{stderr}"))
}

/// Both files failed on the stub's 504, not on a dead stub or a transport error.
fn only_the_barrier_failed(text: &str) {
    assert!(
        text.contains("failed: 2 (2 assertion)") && text.contains("actual:   504"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_feature_files_dispatch_to_the_plugin_at_the_same_time() {
    let bin = require_bddkit!();
    let (code, text) = run_pair(&bin, "barrier-two", 2, "").await;
    assert_eq!(code, 0, "both files must be in flight together\n{text}");
    assert!(
        text.contains("files: 2") && text.contains("failed: 0"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_worker_cannot_release_the_api_barrier() {
    let bin = require_bddkit!();
    let (code, text) = run_pair(&bin, "barrier-one", 1, "").await;
    assert_eq!(code, 1, "one worker must hit the barrier timeout\n{text}");
    only_the_barrier_failed(&text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_files_in_one_serial_chain_never_reach_the_api_together() {
    let bin = require_bddkit!();
    let (code, text) = run_pair(&bin, "barrier-serial", 2, "@serial(shared)\n").await;
    assert_eq!(code, 1, "a chain runs its files one after another\n{text}");
    only_the_barrier_failed(&text);
}
