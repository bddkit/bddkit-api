//! What a tester sees of the plugin's configuration: `doctor`, `resource` and
//! the refusals of a bad config, through the real `bddkit` binary.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{CLOSED, PING, bddkit, combined, project};

fn path(config: &std::path::Path) -> &str {
    config.to_str().expect("UTF-8 path")
}

const TWO_APIS: &str = "  api:\n    main:\n      base_url: http://127.0.0.1:8080\n    other:\n      base_url: http://127.0.0.1:8081\n      timeout_secs: 5\n  srp:\n    main:\n      variant: hex-string\n";

#[test]
fn doctor_shows_each_api_and_srp_instance_statically() {
    let bin = require_bddkit!();
    let config = project("rows", PING, TWO_APIS, "default_api: main\n");
    let out = bddkit(&bin, &["doctor", "--config", path(&config)]);
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("api main"), "{text}");
    assert!(
        text.contains("http://127.0.0.1:8080 (20s timeout)"),
        "{text}"
    );
    assert!(
        text.contains("http://127.0.0.1:8081 (5s timeout)"),
        "{text}"
    );
    assert!(text.contains("srp main"), "{text}");
    assert!(text.contains("hex-string"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_live_reports_the_status_of_a_reachable_base_url() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let resources = format!(
        "  api:\n    main:\n      base_url: {base}\n  srp:\n    main:\n      variant: rfc5054\n"
    );
    let config = project("live-ok", PING, &resources, "");
    let out = tokio::task::spawn_blocking(move || {
        bddkit(&bin, &["doctor", "--live", "--config", path(&config)])
    })
    .await
    .expect("join");
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(&format!("GET {base} → 404")), "{text}");
}

#[test]
fn doctor_live_reports_an_unreachable_base_url() {
    let bin = require_bddkit!();
    let resources = format!("  api:\n    main:\n      base_url: {CLOSED}\n");
    let config = project("live-dead", PING, &resources, "");
    let out = bddkit(&bin, &["doctor", "--live", "--config", path(&config)]);
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("✗ api main"), "{text}");
    assert!(
        !text.contains("--live"),
        "the hint belongs to a static run only:\n{text}"
    );
}

#[test]
fn a_default_authorization_value_is_masked_in_a_refusal_and_in_a_row() {
    let bin = require_bddkit!();
    // The token is in the URL too, so both the refusal and the static row quote it.
    let refused = "  api:\n    main:\n      base_url: tok-abcdefgh\n      default_headers:\n        Authorization: Bearer tok-abcdefgh\n";
    let config = project("mask-refused", PING, refused, "");
    let text = combined(&bddkit(&bin, &["doctor", "--config", path(&config)]));
    assert!(text.contains("invalid base_url"), "{text}");
    assert!(!text.contains("tok-abcdefgh"), "{text}");

    let row = "  api:\n    main:\n      base_url: http://127.0.0.1:8080/tok-abcdefgh\n      default_headers:\n        Authorization: Bearer tok-abcdefgh\n";
    let config = project("mask-row", PING, row, "");
    let text = combined(&bddkit(&bin, &["doctor", "--config", path(&config)]));
    assert!(text.contains("api main"), "{text}");
    assert!(!text.contains("tok-abcdefgh"), "{text}");
}

#[test]
fn a_sensitive_default_header_is_masked_only_under_the_effective_names() {
    let bin = require_bddkit!();
    let resources = "  api:\n    main:\n      base_url: hidden-value\n      sensitive_headers_only: [x-trace]\n      default_headers:\n        Authorization: Bearer visible-token\n        X-Trace: hidden-value\n";
    let config = project("mask-only", PING, resources, "");
    let text = combined(&bddkit(&bin, &["doctor", "--config", path(&config)]));
    assert!(text.contains("invalid base_url"), "{text}");
    assert!(!text.contains("hidden-value"), "{text}");
}

#[test]
fn a_bad_config_is_refused_with_the_instance_named() {
    let bin = require_bddkit!();
    let resources =
        "  api:\n    main:\n      base_url: http://127.0.0.1:8080\n      timeout_secs: 0\n";
    let config = project("zero", PING, resources, "");
    let out = bddkit(&bin, &["run", "--config", path(&config)]);
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("resources.api.main: timeout_secs is 0"),
        "{text}"
    );
}

// core: a_config_without_any_api_resource_fails_at_first_http_step
#[test]
fn a_config_without_any_api_resource_fails_at_the_first_http_step() {
    let bin = require_bddkit!();
    let config = project("no-api", PING, "  api: {}\n", "");
    let out = bddkit(&bin, &["run", "--config", path(&config)]);
    let text = combined(&out);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an ordinary step failure\n{text}"
    );
    assert!(
        text.contains("no instance of the resource group \"api\" is selected"),
        "{text}"
    );
}

#[test]
fn the_fields_of_both_groups_are_listed() {
    let bin = require_bddkit!();
    let config = project("fields", PING, "  api: {}\n  srp: {}\n", "");
    let text = combined(&bddkit(
        &bin,
        &["resource", "fields", "--config", path(&config)],
    ));
    for key in [
        "base_url",
        "timeout_secs",
        "default_headers",
        "sensitive_headers_only",
        "variant",
        "generator",
    ] {
        assert!(text.contains(key), "{key} missing:\n{text}");
    }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_add_inserts_exactly_the_block_and_probes_the_new_api() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let resources = format!("  api:\n    stub:\n      base_url: {base}\n");
    let config = project("add-ok", PING, &resources, "default_api: stub\n");
    let before = std::fs::read_to_string(&config).expect("read");
    let (c, b) = (config.clone(), base.clone());
    let out = tokio::task::spawn_blocking(move || {
        add(
            &bin,
            &c,
            "staging",
            &["--base_url", &b, "--timeout_secs", "5"],
        )
    })
    .await
    .expect("join");
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    assert_eq!(
        std::fs::read_to_string(&config).expect("read"),
        before.replace(
            "  api:\n",
            &format!("  api:\n    staging:\n      base_url: {base}\n      timeout_secs: 5\n")
        )
    );
}

#[test]
fn resource_add_writes_nothing_when_the_probe_fails_and_no_check_skips_it() {
    let bin = require_bddkit!();
    let resources = format!("  api:\n    stub:\n      base_url: {CLOSED}\n");
    let config = project("add-dead", PING, &resources, "default_api: stub\n");
    let before = std::fs::read_to_string(&config).expect("read");
    let out = add(&bin, &config, "staging", &["--base_url", CLOSED]);
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!("  base_url: {CLOSED}\n")),
        "the block is printed: {text}"
    );
    assert_eq!(std::fs::read_to_string(&config).expect("read"), before);

    let out = add(
        &bin,
        &config,
        "staging",
        &["--base_url", CLOSED, "--no-check"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    assert!(
        std::fs::read_to_string(&config)
            .expect("read")
            .contains("staging:")
    );
}

#[test]
fn resource_add_refuses_a_taken_name_and_a_misspelled_field() {
    let bin = require_bddkit!();
    let resources = format!("  api:\n    stub:\n      base_url: {CLOSED}\n");
    let config = project("add-refused", PING, &resources, "default_api: stub\n");
    let before = std::fs::read_to_string(&config).expect("read");

    let out = add(
        &bin,
        &config,
        "stub",
        &["--base_url", "http://other.local", "--no-check"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));

    let out = add(
        &bin,
        &config,
        "staging",
        &["--base_rul", "http://a.local", "--no-check"],
    );
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("base_rul"), "{text}");
    assert_eq!(std::fs::read_to_string(&config).expect("read"), before);
}
