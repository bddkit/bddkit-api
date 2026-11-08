//! What a run and a `doctor` record about the `api` resources of a suite. The
//! host has no `resources.api` key with a plugin serving `api`; these pin what
//! it records instead: the instances of a plugin's group, counted under the
//! plugin's name (`resources.plugins`), and only for a plugin its lock names
//! as official (`source: bddkit/...`). The payload it would send is read back
//! from `BDDKIT_TELEMETRY_LOG`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::combined;
use common::ported::Suite;

const OFFICIAL: &str = "    source: bddkit/bddkit-api\n    version: 0.1.0\n";
const TWO_APIS: &str = "default_api: stub\npaths: [features]\nresources:\n  api:\n    stub:\n      base_url: http://127.0.0.1:9\n    other:\n      base_url: http://127.0.0.1:9\n";

// core: a_run_counts_the_api_resources_of_the_suite, whose `resources.api` key is
// host-owned; this pins the count the host keeps for a plugin's group instead.
#[test]
fn a_run_counts_the_api_instances_under_the_plugin() {
    let bin = require_bddkit!();
    let suite = Suite::new("api-count", TWO_APIS, OFFICIAL);
    let out = suite.bddkit(&bin, &["run", "--telemetry", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    let payload = suite.only_payload();
    assert_eq!(
        payload["resources"]["plugins"],
        serde_json::json!({"api": 2})
    );
    assert_eq!(payload["resources"]["plugins_other"], 0);
}

// core: doctor_counts_the_api_resources_of_the_suite, see above.
#[test]
fn doctor_counts_the_api_instances_under_the_plugin() {
    let bin = require_bddkit!();
    let suite = Suite::new("doctor-api-count", TWO_APIS, OFFICIAL);
    let out = suite.bddkit(&bin, &["doctor", "--json", "--telemetry", "true"]);
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    let payload = suite.only_payload();
    assert_eq!(payload["command"], "doctor");
    assert_eq!(
        payload["resources"]["plugins"],
        serde_json::json!({"api": 2})
    );
}
