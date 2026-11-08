//! Every reply the plugin sends is built here, so the shape the host parses
//! has one author.

use std::collections::BTreeMap;

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
    Assertion,
    /// The clock or the system entropy failed: neither the scenario nor the
    /// service is at fault, so no other class fits.
    Unclassified,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Class::Input => "input",
            Class::Transport => "transport",
            Class::Assertion => "assertion",
            Class::Unclassified => "unclassified",
        }
    }
}

/// A failure: `not_yet` when a later attempt may see it hold, `fatal` when
/// nothing can change.
struct Failure {
    retry: bool,
    class: Class,
    error: String,
}

/// What a reply writes besides its verdict.
#[derive(Default)]
struct Written {
    /// Variables a passed reply writes, to the scenario's frame.
    vars: BTreeMap<String, String>,
    /// Variables a passed reply writes through `global_vars`, which the host
    /// warns about when a file-level variable of the name shadows them.
    global_vars: BTreeMap<String, String>,
    /// Lines the host prints to stderr, whatever the status.
    output: Vec<Value>,
}

/// A dispatch reply under construction. `secrets` and `scan` go out on every
/// status, a failure's included: that is where a value is most likely quoted.
#[derive(Default)]
pub struct Reply {
    failure: Option<Box<Failure>>,
    /// On a `not_yet`: the instance whose polling options govern the poll.
    options_instance: Option<String>,
    pub secrets: Vec<String>,
    pub scan: Vec<Value>,
    pub diagnostics: Vec<Value>,
    /// Boxed: `Reply` is the `Err` of most steps, and a large one is a lint.
    written: Option<Box<Written>>,
}

impl Reply {
    pub fn passed() -> Self {
        Self::default()
    }

    fn written(&mut self) -> &mut Written {
        self.written.get_or_insert_with(Box::default)
    }

    /// Writes a variable once the reply passes; `global` for the whole file.
    pub fn set_var(&mut self, name: &str, value: String, global: bool) {
        let written = self.written();
        let vars = if global {
            &mut written.global_vars
        } else {
            &mut written.vars
        };
        vars.insert(name.to_string(), value);
    }

    /// An item the host prints to stderr, on any status.
    pub fn print(&mut self, item: Value) {
        self.written().output.push(item);
    }

    pub fn failed(class: Class, error: impl Into<String>) -> Self {
        Self::passed().into_failure(class, error)
    }

    /// The same registrations under a failure.
    pub fn into_failure(self, class: Class, error: impl Into<String>) -> Self {
        Self {
            failure: Some(Box::new(Failure {
                retry: false,
                class,
                error: error.into(),
            })),
            ..self
        }
    }

    /// The same registrations under a failure a later attempt may not repeat.
    /// The poll takes its options from `options_instance`.
    pub fn into_not_yet(
        self,
        class: Class,
        error: impl Into<String>,
        options_instance: &str,
    ) -> Self {
        Self {
            failure: Some(Box::new(Failure {
                retry: true,
                class,
                error: error.into(),
            })),
            options_instance: Some(options_instance.to_string()),
            ..self
        }
    }

    /// The class and message of a failure, if the reply is one.
    pub fn failure(&self) -> Option<(Class, &str)> {
        self.failure.as_ref().map(|f| (f.class, f.error.as_str()))
    }

    pub fn json(&self) -> String {
        let mut reply = match &self.failure {
            None => json!({"status": "passed"}),
            Some(f) => json!({
                "status": if f.retry { "not_yet" } else { "fatal" },
                "error": f.error,
                "class": f.class.as_str(),
            }),
        };
        if let Some(instance) = &self.options_instance {
            reply["options_instance"] = json!(instance);
        }
        reply["secrets"] = json!(self.secrets);
        reply["scan"] = json!(self.scan);
        reply["diagnostics"] = json!(self.diagnostics);
        if let Some(written) = &self.written {
            reply["output"] = json!(written.output);
            if self.failure.is_none() {
                reply["vars"] = json!(written.vars);
                reply["global_vars"] = json!(written.global_vars);
            }
        }
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
    fn variables_are_sent_with_a_pass_and_dropped_from_a_failure() {
        let mut reply = Reply::passed();
        reply.set_var("a", "1".into(), false);
        reply.set_var("b", "2".into(), true);
        reply.print(json!({"text": "t", "syntax": null}));
        let v: Value = serde_json::from_str(&reply.json()).expect("JSON");
        assert_eq!(v["vars"], json!({"a": "1"}));
        assert_eq!(v["global_vars"], json!({"b": "2"}));
        let v: Value =
            serde_json::from_str(&reply.into_failure(Class::Input, "no").json()).expect("JSON");
        assert!(v.get("vars").is_none() && v.get("global_vars").is_none());
        assert_eq!(v["output"], json!([{"text": "t", "syntax": null}]));
    }

    #[test]
    fn a_not_yet_names_the_instance_whose_options_govern_the_poll() {
        let v: Value = serde_json::from_str(
            &Reply::passed()
                .into_not_yet(Class::Assertion, "later", "main")
                .json(),
        )
        .expect("JSON");
        assert_eq!(v["status"], "not_yet");
        assert_eq!(v["class"], "assertion");
        assert_eq!(v["options_instance"], "main");
        let fatal: Value =
            serde_json::from_str(&Reply::failed(Class::Input, "no").json()).expect("JSON");
        assert!(fatal.get("options_instance").is_none());
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

    #[test]
    fn the_host_faults_are_unclassified() {
        let v: Value = serde_json::from_str(&Reply::failed(Class::Unclassified, "no clock").json())
            .expect("JSON");
        assert_eq!(
            (v["status"].as_str(), v["class"].as_str()),
            (Some("fatal"), Some("unclassified"))
        );
    }
}
