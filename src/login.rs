//! The four SRP steps. The client's private values live in the file's
//! variables, never in the handle: `<prefix>_a` is written by the start and
//! read back by the completion, so it keeps the variables' scoping.

use crate::reply::{Class, Reply};
use crate::srp::{self, SrpParams};

/// The system entropy failing is a host fault: no other class fits.
fn host(error: &srp::Entropy) -> Reply {
    Reply::failed(Class::Unclassified, error.to_string())
}

/// The salt is the one the step received, or a fresh one.
pub fn generate_verifier(
    params: &SrpParams,
    identity: &str,
    password: &str,
    salt: Option<&str>,
    prefix: &str,
) -> Reply {
    let salt = match salt {
        Some(salt) => salt.to_string(),
        None => match srp::generate_salt() {
            Ok(salt) => salt,
            Err(error) => return host(&error),
        },
    };
    match srp::compute_verifier(params, &salt, identity, password) {
        Ok(verifier) => {
            let mut reply = Reply::passed();
            reply.set_var(&format!("{prefix}_salt"), salt, false);
            reply.set_var(&format!("{prefix}_verifier"), verifier, false);
            reply
        }
        Err(error) => Reply::failed(Class::Input, error),
    }
}

/// The private `a` is secret whatever the prefix is called.
pub fn start_login(params: &SrpParams, prefix: &str) -> Reply {
    match srp::start_login(params) {
        Ok((a, a_pub)) => {
            let mut reply = Reply::passed();
            reply.secrets.push(a.clone());
            reply.set_var(&format!("{prefix}_a"), a, false);
            reply.set_var(&format!("{prefix}_A"), a_pub, false);
            reply
        }
        Err(error) => host(&error),
    }
}

/// What `complete_login` is given beyond the parameters.
pub struct Completion<'a> {
    pub prefix: &'a str,
    pub identity: &'a str,
    pub password: &'a str,
    pub salt: &'a str,
    pub server_public: &'a str,
    /// `<prefix>_a`, if the variables hold it.
    pub a: Option<&'a str>,
}

pub fn complete_login(params: &SrpParams, call: &Completion) -> Reply {
    let prefix = call.prefix;
    let Some(a) = call.a else {
        return Reply::failed(
            Class::Input,
            format!(
                "run the step \"I start an SRP login as {prefix:?}\" before completing the login"
            ),
        );
    };
    match srp::complete_login(
        params,
        call.salt,
        call.identity,
        call.password,
        a,
        call.server_public,
    ) {
        Ok(proof) => {
            let mut reply = Reply::passed();
            reply.secrets.push(proof.session_key.clone());
            reply.set_var(&format!("{prefix}_M1"), proof.m1, false);
            reply.set_var(&format!("{prefix}_M2"), proof.m2, false);
            reply.set_var(&format!("{prefix}_sessionKey"), proof.session_key, false);
            reply
        }
        Err(error) => Reply::failed(Class::Input, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn params() -> SrpParams {
        SrpParams::from_config(&json!({"variant": "hex-string", "generator": "5"})).expect("valid")
    }

    fn written(reply: &Reply) -> Value {
        serde_json::from_str(&reply.json()).expect("JSON")
    }

    #[test]
    fn a_verifier_writes_the_salt_and_the_verifier_under_the_prefix() {
        let v = written(&generate_verifier(
            &params(),
            "u@example.test",
            "pw",
            None,
            "reg",
        ));
        assert_eq!(v["status"], "passed");
        assert_eq!(v["vars"]["reg_salt"].as_str().map(str::len), Some(64));
        assert!(v["vars"]["reg_verifier"].is_string(), "{v}");
        assert_eq!(v["secrets"], json!([]));
    }

    #[test]
    fn a_supplied_salt_is_echoed_and_gives_the_same_verifier_each_time() {
        let a = written(&generate_verifier(
            &params(),
            "u",
            "pw",
            Some("0abc"),
            "one",
        ));
        let b = written(&generate_verifier(
            &params(),
            "u",
            "pw",
            Some("0abc"),
            "two",
        ));
        assert_eq!(a["vars"]["one_salt"], "0abc");
        assert_eq!(a["vars"]["one_verifier"], b["vars"]["two_verifier"]);
    }

    #[test]
    fn starting_a_login_writes_both_values_and_keeps_the_private_one_secret() {
        let v = written(&start_login(&params(), "srp"));
        let a = v["vars"]["srp_a"].as_str().expect("a");
        assert!(v["vars"]["srp_A"].is_string());
        assert_eq!(v["secrets"], json!([a]));
    }

    #[test]
    fn completing_without_a_start_names_the_step_to_run_first() {
        let v = written(&complete_login(
            &params(),
            &Completion {
                prefix: "srp",
                identity: "u",
                password: "pw",
                salt: "aa",
                server_public: "bb",
                a: None,
            },
        ));
        assert_eq!(
            (v["status"].as_str(), v["class"].as_str()),
            (Some("fatal"), Some("input"))
        );
        assert_eq!(
            v["error"],
            r#"run the step "I start an SRP login as "srp"" before completing the login"#
        );
    }

    #[test]
    fn completing_writes_the_proofs_and_keeps_the_session_key_secret() {
        let p = params();
        let registered = written(&generate_verifier(&p, "u", "pw", None, "reg"));
        let salt = registered["vars"]["reg_salt"].as_str().expect("salt");
        let verifier = registered["vars"]["reg_verifier"]
            .as_str()
            .expect("verifier");
        let started = written(&start_login(&p, "srp"));
        let a = started["vars"]["srp_a"].as_str().expect("a");
        let v = num_bigint::BigUint::parse_bytes(verifier.as_bytes(), 16).expect("hex");
        let b_priv = num_bigint::BigUint::from(0x5c2e_91a0_u32);
        let b_pub = (p.k() * v + p.generator.modpow(&b_priv, &p.prime)) % &p.prime;
        let v = written(&complete_login(
            &p,
            &Completion {
                prefix: "srp",
                identity: "u",
                password: "pw",
                salt,
                server_public: &srp::hex(&b_pub),
                a: Some(a),
            },
        ));
        assert_eq!(v["status"], "passed", "{v}");
        for name in ["srp_M1", "srp_M2", "srp_sessionKey"] {
            assert_eq!(v["vars"][name].as_str().map(str::len), Some(64), "{name}");
        }
        assert_eq!(v["secrets"], json!([v["vars"]["srp_sessionKey"]]));
    }
}
