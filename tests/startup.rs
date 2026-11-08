//! What `run` refuses before the first request, for the resources this plugin
//! owns.

mod common;

use common::{CLOSED, PING, project, run};

/// Every declared SRP resource is validated at startup, not just the default
/// one. Otherwise a broken `variant:` in a second block sits there until
/// someone points `default_srp` at it — and `doctor`, which reports on every
/// declared resource, would be stricter than the run it is meant to predict.
#[test]
fn run_refuses_a_malformed_srp_resource_that_is_not_the_default() {
    let bin = require_bddkit!();
    let resources = format!(
        "  api:\n    stub:\n      base_url: {CLOSED}\n  srp:\n    good:\n      variant: hex-string\n    legacy:\n      variant: bogus\n"
    );
    let cfg = project("run-srp", PING, &resources, "default_srp: good\n");

    let (code, stdout, stderr) = run(&bin, &cfg, &[]);

    assert_eq!(
        code, 2,
        "a malformed resource is a startup failure, not a scenario failure\n{stdout}{stderr}"
    );
    assert!(
        stderr.contains("legacy"),
        "the bad resource is named:\n{stderr}"
    );
}
