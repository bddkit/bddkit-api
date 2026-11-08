//! Every reply the plugin sends is built here, so the shape the host parses
//! has one author.

use serde_json::json;

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

/// Retrying cannot help.
pub fn fatal(error: &str) -> String {
    json!({"status": "fatal", "error": error}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
