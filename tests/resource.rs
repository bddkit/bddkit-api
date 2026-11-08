//! `bddkit resource` on the `api` and `srp` kinds, ported from the host's
//! suite. A plugin's kind is listed once the config names its group.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::ported::path;
use common::{CLOSED, PING, api_project, bddkit, combined, project};

fn config() -> std::path::PathBuf {
    project("fields-listing", PING, "  api: {}\n  srp: {}\n", "")
}

#[test]
fn the_listing_covers_every_kind_the_host_serves() {
    let bin = require_bddkit!();
    let out = bddkit(&bin, &["resource", "fields", "--config", path(&config())]);
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("api:"), "{text}");
    assert!(text.contains("srp:"), "{text}");
    assert!(text.contains("base_url"), "{text}");
    assert!(text.contains("variant"), "{text}");
    assert!(
        text.contains("required"),
        "a reader has to be able to tell the mandatory keys apart:\n{text}"
    );
}

#[test]
fn json_carries_every_key_of_the_field_description() {
    let bin = require_bddkit!();
    let out = bddkit(
        &bin,
        &[
            "resource",
            "fields",
            "api",
            "--json",
            "--config",
            path(&config()),
        ],
    );
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let kinds: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let kinds = kinds.as_array().expect("an array");
    assert_eq!(kinds.len(), 1);
    let api = &kinds[0];
    assert_eq!(api["kind"], "api");
    assert_eq!(api["source"], "plugin");
    let fields = api["fields"].as_array().expect("an array of fields");
    let base_url = fields
        .iter()
        .find(|f| f["name"] == "base_url")
        .expect("base_url is described");
    assert_eq!(base_url["required"], true);
    assert!(base_url["description"].is_string());
}

fn add(
    bin: &std::path::Path,
    config: &std::path::Path,
    name: &str,
    flags: &[&str],
) -> std::process::Output {
    let mut args = vec!["resource", "add", "api", name, "--config", path(config)];
    args.extend_from_slice(flags);
    bddkit(bin, &args)
}

fn dead(name: &str) -> (std::path::PathBuf, String) {
    let config = api_project(name, CLOSED, PING, "", "default_api: stub\n");
    let before = std::fs::read_to_string(&config).expect("read config");
    (config, before)
}

/// The probe is on by default here — the resource being added is what the
/// invocation is about — and a resource that cannot answer is not written.
#[test]
fn resource_add_writes_nothing_when_the_probe_fails() {
    let bin = require_bddkit!();
    let (config, before) = dead("resource-add-dead");
    let out = add(&bin, &config, "staging", &["--base_url", CLOSED]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    // The resource's own line, not the word `base_url` — which the failure
    // message itself carries, so `contains("base_url")` proves nothing.
    assert!(
        text.contains(&format!("  base_url: {CLOSED}\n")),
        "the block is printed: {text}"
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("read config"),
        before,
        "a failed add writes nothing"
    );
}

/// `--no-check` skips the probe and nothing else: the same closed port, and
/// the resource is written.
#[test]
fn resource_add_no_check_writes_a_resource_that_is_not_up_yet() {
    let bin = require_bddkit!();
    let (config, _) = dead("resource-add-nocheck");
    let out = add(
        &bin,
        &config,
        "staging",
        &["--base_url", CLOSED, "--no-check"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    assert!(
        std::fs::read_to_string(&config)
            .expect("read config")
            .contains("staging:"),
        "the resource is written"
    );
}

/// Insert only, and the refusal is what a script reads: exit 1, the block on
/// stdout, the file untouched.
#[test]
fn resource_add_refuses_a_name_that_is_already_taken() {
    let bin = require_bddkit!();
    let (config, before) = dead("resource-add-taken");
    let out = add(
        &bin,
        &config,
        "stub",
        &["--base_url", "http://other.local", "--no-check"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    assert_eq!(std::fs::read_to_string(&config).expect("read"), before);
}

/// A typo in a field name is an error, not a silently stored key — the whole
/// reason the flags are checked against a list at all.
#[test]
fn resource_add_names_a_field_that_does_not_exist() {
    let bin = require_bddkit!();
    let (config, before) = dead("resource-add-typo");
    let out = add(
        &bin,
        &config,
        "staging",
        &["--base_rul", "http://a.local", "--no-check"],
    );
    let text = combined(&out);
    // Exactly 1: this command reports, so `not zero` would also accept a usage
    // error or a panic, which are the two answers it must never give.
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("base_rul"), "{text}");
    assert_eq!(
        std::fs::read_to_string(&config).expect("read config"),
        before,
        "a refused field writes nothing"
    );
}

/// The block a refused `resource add` prints is masked. The core's version
/// added a database DSN; the same rule covers a password in an API's
/// `base_url`, and the refusal is the same: a second API with no default.
#[test]
fn resource_add_masks_the_block_it_prints() {
    let bin = require_bddkit!();
    let config = project(
        "mask-resource-add",
        PING,
        "  api:\n    a:\n      base_url: http://127.0.0.1:1/\n",
        "",
    );
    let out = add(
        &bin,
        &config,
        "second",
        &["--base_url", "http://app:add-pass-5678@127.0.0.1:1/"],
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("The block, for manual insertion"), "{text}");
    assert!(text.contains("app:***@127.0.0.1"), "{text}");
    assert!(!text.contains("add-pass-5678"), "{text}");
}
