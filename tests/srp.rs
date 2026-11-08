//! A non-default SRP resource, reached with `I use "<name>" srp`, seen through
//! the real `bddkit`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "`allow-*-in-tests` covers only `#[test]` functions, not the helpers here"
)]

mod common;

use common::{project, run};

/// Register an identity and log in; the last step's status is `status`.
fn login(identity: &str, status: u16) -> String {
    format!(
        "    When I generate an SRP verifier for \"{identity}\" with password \"secret\" as \"reg\"
    And the request body is:
      \"\"\"
      {{\"identity\": \"{identity}\", \"salt\": \"<<reg_salt>>\", \"verifier\": \"<<reg_verifier>>\"}}
      \"\"\"
    And I request \"/srp/register\" using HTTP POST
    Then the response code is 200
    When I start an SRP login as \"srp\"
    And the request body is:
      \"\"\"
      {{\"identity\": \"{identity}\"}}
      \"\"\"
    And I request \"/srp/step1\" using HTTP POST
    Then the response code is 200
    And extract \"salt\" from JSON as \"serverSalt\"
    And extract \"b\" from JSON as \"serverB\"
    When I complete SRP login \"srp\" for \"{identity}\" with password \"secret\" salt \"<<serverSalt>>\" and \"<<serverB>>\"
    And the request body is:
      \"\"\"
      {{\"identity\": \"{identity}\", \"a\": \"<<srp_A>>\", \"m1\": \"<<srp_M1>>\"}}
      \"\"\"
    And I request \"/srp/step2\" using HTTP POST
    Then the response code is {status}
"
    )
}

/// `wrong` is the default and speaks the other variant, so only the instance
/// the scenario switches to can log in against the stub.
fn resources(base: &str) -> String {
    let prime = common::stub::SRP_PRIME_HEX;
    format!(
        "  api:\n    stub:\n      base_url: {base}\n  srp:\n    wrong:\n      variant: rfc5054\n      generator: 2\n      prime: \"{prime}\"\n    right:\n      variant: hex-string\n      generator: 2\n      prime: \"{prime}\"\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_default_srp_instance_is_reached_by_name_and_the_default_returns_after_it() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = format!(
        "Feature: two srp instances\n  Scenario: the named instance logs in\n    Given I use \"right\" srp\n{}  Scenario: the default does not\n{}",
        login("named@example.test", 200),
        login("default@example.test", 401),
    );
    let cfg = project(
        "two-srp",
        &feature,
        &resources(&base),
        "default_srp: wrong\n",
    );
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("scenarios: 2, failed: 0"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completing_without_a_start_names_the_step_to_run_first_and_shows_no_password() {
    let bin = require_bddkit!();
    let base = common::stub::spawn().await;
    let feature = "Feature: order\n  Scenario: complete without a start\n    When I request \"/ping\"\n    And I complete SRP login \"srp\" for \"u\" with password \"pw-not-shown\" salt \"aa\" and \"bb\"\n";
    let cfg = project(
        "srp-order",
        feature,
        &resources(&base),
        "default_srp: right\n",
    );
    let (code, stdout, stderr) = run(&bin, &cfg, &[]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stdout.contains("before completing the login"),
        "{stdout}{stderr}"
    );
    // A failing SRP step still shows the last exchange.
    assert!(stdout.contains("/ping"), "{stdout}{stderr}");
    assert!(!stdout.contains("pw-not-shown") && !stderr.contains("pw-not-shown"));
}
