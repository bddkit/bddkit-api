//! `bddkit init` without a terminal, with the `api` plugin loaded: every
//! answer from a flag. Ported from the host's suite.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::combined;
use common::ported::{in_dir, temp};
use std::path::Path;

fn config(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("bddkit.yaml")).expect("bddkit.yaml written")
}

#[test]
fn init_with_flags_writes_a_config_that_doctor_accepts() {
    let bin = require_bddkit!();
    let dir = temp("init-doctor");
    let out = in_dir(&bin, &dir, &["init", "--api-url", "http://127.0.0.1:8080"]);
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    let config = config(&dir);
    assert!(
        config.contains("base_url: http://127.0.0.1:8080"),
        "{config}"
    );
    assert!(
        config.contains("# "),
        "every key carries a comment:\n{config}"
    );
    let doctor = in_dir(&bin, &dir, &["doctor"]);
    assert_eq!(doctor.status.code(), Some(0), "{}", combined(&doctor));
}

#[test]
fn the_example_feature_passes_without_a_network() {
    let bin = require_bddkit!();
    let dir = temp("init-example");
    // Nothing listens on this port: the request in the feature is a comment.
    let out = in_dir(&bin, &dir, &["init", "--api-url", "http://127.0.0.1:9"]);
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    let feature = std::fs::read_to_string(dir.join("features/example.feature")).expect("feature");
    assert!(feature.contains("# When I request \"/\""), "{feature}");
    let run = in_dir(&bin, &dir, &["run"]);
    assert_eq!(run.status.code(), Some(0), "{}", combined(&run));
}

#[test]
fn an_empty_api_answer_writes_no_api_resource() {
    let bin = require_bddkit!();
    let dir = temp("init-no-api");
    let out = in_dir(&bin, &dir, &["init", "--no-api"]);
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    assert!(!config(&dir).contains("base_url"), "{}", config(&dir));
    let feature = std::fs::read_to_string(dir.join("features/example.feature")).unwrap();
    assert!(!feature.contains("request"), "{feature}");
    // No flags at all means the same.
    let bare = temp("init-bare");
    assert_eq!(in_dir(&bin, &bare, &["init"]).status.code(), Some(0));
    assert!(!config(&bare).contains("base_url"));
}
