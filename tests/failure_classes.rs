//! Why a scenario failed, as data: the class of an HTTP failure in the console
//! summary. Ported from the core's `failure_classes.rs`; the classes that need
//! no HTTP stay there.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{api_project, run};

/// The console's summary line of a run that failed.
fn summary(bin: &std::path::Path, config: &std::path::Path) -> String {
    let (code, stdout, _) = run(bin, config, &[]);
    assert_eq!(code, 1, "{stdout}");
    stdout
        .lines()
        .find(|line| line.starts_with("files:"))
        .unwrap_or_else(|| panic!("no summary line in {stdout}"))
        .to_string()
}

#[test]
fn a_refused_connection_is_a_transport_failure() {
    let bin = require_bddkit!();
    // Port 1 on loopback: nothing listens, the connection is refused.
    let cfg = api_project(
        "class-refused",
        "http://127.0.0.1:1",
        "Feature: f\n  Scenario: s\n    When I request \"/ping\"\n",
        "      timeout_secs: 2\n",
        "",
    );
    assert!(summary(&bin, &cfg).ends_with("failed: 1 (1 transport)"));
}
