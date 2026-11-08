//! Every reply the plugin sends is built here, so the shape the host parses
//! has one author.

use serde_json::{Value, json};

/// The envelope shared by validate / probe / init / drop / select / reset.
pub fn ok() -> String {
    r#"{"ok":true}"#.to_string()
}

pub fn err(error: impl Into<String>) -> String {
    json!({"ok": false, "error": error.into()}).to_string()
}

/// A config reply: the one-line `detail` for `doctor` on success, the error on
/// refusal, and in both cases the values of the config that must never be
/// printed.
pub fn config(result: Result<String, String>, secrets: &[String]) -> String {
    match result {
        Ok(detail) => json!({"ok": true, "detail": detail, "secrets": secrets}),
        Err(error) => json!({"ok": false, "error": error, "secrets": secrets}),
    }
    .to_string()
}

/// A refusal that names the instance, which the host reports as
/// `resources.<group>.<instance>: <error>`.
pub fn err_in(instance: &str, error: impl Into<String>) -> String {
    json!({"ok": false, "instance": instance, "error": error.into()}).to_string()
}

pub fn handle(handle: u64) -> String {
    json!({"ok": true, "handle": handle}).to_string()
}

/// Retrying cannot help, and no class fits: a fault of the call itself.
pub fn fatal(error: &str) -> String {
    json!({"status": "fatal", "error": error}).to_string()
}

/// The failure classes this plugin names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Input,
    Transport,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Class::Input => "input",
            Class::Transport => "transport",
        }
    }
}

/// A dispatch reply under construction. `secrets` and `scan` go out on every
/// status, a failure's included: that is where a value is most likely quoted.
#[derive(Default)]
pub struct Reply {
    failure: Option<(Class, String)>,
    pub secrets: Vec<String>,
    pub scan: Vec<Value>,
    pub diagnostics: Vec<Value>,
}

impl Reply {
    pub fn passed() -> Self {
        Self::default()
    }

    pub fn failed(class: Class, error: impl Into<String>) -> Self {
        Self {
            failure: Some((class, error.into())),
            ..Self::default()
        }
    }

    /// The same registrations under a failure.
    pub fn into_failure(self, class: Class, error: impl Into<String>) -> Self {
        Self {
            failure: Some((class, error.into())),
            ..self
        }
    }

    pub fn json(&self) -> String {
        let mut reply = match &self.failure {
            None => json!({"status": "passed"}),
            Some((class, error)) => {
                json!({"status": "fatal", "error": error, "class": class.as_str()})
            }
        };
        reply["secrets"] = json!(self.secrets);
        reply["scan"] = json!(self.scan);
        reply["diagnostics"] = json!(self.diagnostics);
        reply.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_failure_keeps_its_registrations_and_names_its_class() {
        let mut reply = Reply::passed();
        reply.secrets.push("s3cr3t".into());
        let v: Value = serde_json::from_str(&reply.into_failure(Class::Transport, "down").json())
            .expect("JSON");
        assert_eq!(v["status"], "fatal");
        assert_eq!(v["class"], "transport");
        assert_eq!(v["secrets"], json!(["s3cr3t"]));
    }

    #[test]
    fn a_refusal_carries_its_secrets_too() {
        let v: Value = serde_json::from_str(&config(Err("no".into()), &["s3cr3t-value".into()]))
            .expect("JSON");
        assert_eq!(v["ok"], false);
        assert_eq!(v["secrets"], json!(["s3cr3t-value"]));
    }

    #[test]
    fn a_refusal_names_its_instance() {
        let v: Value = serde_json::from_str(&err_in("audit", "boom")).expect("JSON");
        assert_eq!(v["instance"], "audit");
    }
}
