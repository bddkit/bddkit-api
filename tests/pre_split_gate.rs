//! The release gate (bddkit-api#20): the core's suites as they were at the
//! `pre-split` tag pass, unmodified, on the candidate host with this plugin.
//! `PRE_SPLIT_DIR` is a checkout of `bddkit/bddkit` at that tag's commit. The tests are
//! `#[ignore]`d so a plain `cargo test` does not need it, and they panic
//! without it so the gate job can never go green by running nothing.
//! `BDDKIT_API_LIBRARY` makes them run a built release library.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use std::path::PathBuf;

fn pre_split_dir() -> PathBuf {
    let dir = std::env::var_os("PRE_SPLIT_DIR")
        .expect("PRE_SPLIT_DIR must name a checkout of bddkit/bddkit at pre-split");
    PathBuf::from(dir)
}

/// `bddkit run --config <config>` inside the checkout, whose paths are relative.
fn run_suite(config: &str, stub: Option<&str>, files: usize) {
    let bin = require_bddkit!();
    let mut command = common::command(&bin, &["run", "--config", config]);
    command.current_dir(pre_split_dir());
    if let Some(stub) = stub {
        command.env("BDDKIT_STUB_URL", stub);
    }
    let out = command.output().expect("bddkit");
    let text = common::combined(&out);
    assert!(out.status.success(), "{text}");
    // Counted so a config that selects nothing cannot pass.
    assert!(
        text.contains(&format!("files: {files},")) && text.contains("failed: 0"),
        "{text}"
    );
}

/// The API scenarios of the core, against this repository's stub.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release gate: needs PRE_SPLIT_DIR"]
async fn pre_split_api_features_pass_against_the_stub() {
    let base = common::stub::spawn().await;
    // Blocking `Command::output` on the runtime's thread would starve the stub.
    tokio::task::spawn_blocking(move || run_suite("tests/api/acceptance.yaml", Some(&base), 8))
        .await
        .expect("join");
}

/// The core's own scenarios (includes, variables) with the plugin installed:
/// loading it must change nothing about them.
#[test]
#[ignore = "release gate: needs PRE_SPLIT_DIR"]
fn pre_split_core_features_pass_with_the_plugin_installed() {
    run_suite("tests/acceptance.yaml", None, 13);
}
