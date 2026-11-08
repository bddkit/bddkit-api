//! One `resources.api.<name>` or `resources.srp.<name>` body, as the host
//! hands it over verbatim minus its reserved `options` key. Unknown keys are
//! ignored.

use std::collections::BTreeMap;

use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::header;

const DEFAULT_TIMEOUT_SECS: u64 = 20;

/// What `resources.api.<name>` takes.
#[derive(Debug, Deserialize)]
struct RawApi {
    base_url: String,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    #[serde(default)]
    default_headers: BTreeMap<String, String>,
    #[serde(default)]
    sensitive_headers: Option<Vec<String>>,
    #[serde(default)]
    sensitive_headers_only: Option<Vec<String>>,
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

/// A checked API resource.
#[derive(Debug)]
pub struct Api {
    pub base_url: Url,
    /// The text as written: `doctor` shows it, not the parsed form.
    pub base_url_text: String,
    pub timeout_secs: u64,
    pub default_headers: BTreeMap<String, String>,
    /// Lowercase names of the headers whose values are masked.
    pub sensitive_headers: Vec<String>,
}

impl Api {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let raw = RawApi::deserialize(config).map_err(|e| e.to_string())?;
        // `reqwest` accepts a zero timeout and then fails every request
        // instantly, which reads as an unreachable service rather than as the
        // config error it is.
        if raw.timeout_secs == 0 {
            return Err("timeout_secs is 0: every request would time out immediately".into());
        }
        let base_url = Url::parse(&raw.base_url)
            .map_err(|e| format!("invalid base_url {:?}: {e}", raw.base_url))?;
        // `localhost:8080` parses (as scheme `localhost`) and then fails at
        // the first request with an error that never mentions the config.
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.has_host() {
            return Err(format!(
                "invalid base_url {:?}: expected an http:// or https:// URL with a host",
                raw.base_url
            ));
        }
        let mut sensitive_headers: Vec<String> = header::DEFAULT_SENSITIVE
            .iter()
            .map(|n| (*n).to_string())
            .collect();
        fold_headers(
            &mut sensitive_headers,
            raw.sensitive_headers.as_deref(),
            raw.sensitive_headers_only.as_deref(),
        )?;
        Ok(Self {
            base_url,
            base_url_text: raw.base_url,
            timeout_secs: raw.timeout_secs,
            default_headers: raw.default_headers,
            sensitive_headers,
        })
    }

    /// The static line `doctor` shows.
    pub fn detail(&self) -> String {
        format!("{} ({}s timeout)", self.base_url_text, self.timeout_secs)
    }
}

/// `sensitive_headers_only` replaces the list, `sensitive_headers` adds to it,
/// and asking for both is a refusal. An empty `only` masks no header.
fn fold_headers(
    list: &mut Vec<String>,
    extend: Option<&[String]>,
    only: Option<&[String]>,
) -> Result<(), String> {
    if extend.is_some() && only.is_some() {
        return Err(
            "sensitive_headers and sensitive_headers_only are both set; use \
                    sensitive_headers_only to replace the list, sensitive_headers to add to it"
                .into(),
        );
    }
    let normalised = |values: &[String], key: &str| -> Result<Vec<String>, String> {
        values
            .iter()
            .map(|value| {
                header::name(value)
                    .ok_or_else(|| format!("{key}: {value:?} is not a valid header name"))
            })
            .collect()
    };
    if let Some(only) = only {
        *list = normalised(only, "sensitive_headers_only")?;
    }
    if let Some(extend) = extend {
        for value in normalised(extend, "sensitive_headers")? {
            if !list.contains(&value) {
                list.push(value);
            }
        }
    }
    Ok(())
}

/// The values a config holds that must never be printed: every default header
/// under a sensitive name, whole and split. Read from the raw body so a refusal
/// masks them too, and so a config that is wrong in another key still does.
/// Where `sensitive_headers` and `sensitive_headers_only` are both set, the
/// union of the two is used: the refusal quotes neither, but masking more is
/// the safe side.
pub fn secrets(group: &str, config: &Value) -> Vec<String> {
    if group != "api" {
        return Vec::new();
    }
    let names = |key: &str| -> Option<Vec<String>> {
        config.get(key)?.as_array().map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect()
        })
    };
    let (extend, only) = (names("sensitive_headers"), names("sensitive_headers_only"));
    let mut sensitive: Vec<String> = match (&extend, &only) {
        (None, Some(only)) => only.clone(),
        _ => header::DEFAULT_SENSITIVE
            .iter()
            .map(|n| (*n).to_string())
            .collect(),
    };
    sensitive.extend(extend.into_iter().chain(only).flatten());
    // Only the password is a credential (the host masks the same); it shows in the doctor row and in transport errors.
    let mut found: Vec<String> = config
        .get("base_url")
        .and_then(Value::as_str)
        .and_then(|text| Url::parse(text).ok())
        .and_then(|url| url.password().map(str::to_string))
        .into_iter()
        .collect();
    let Some(headers) = config.get("default_headers").and_then(Value::as_object) else {
        return found;
    };
    found.extend(
        headers
            .iter()
            .filter_map(|(name, value)| Some((name.to_ascii_lowercase(), value.as_str()?)))
            .filter(|(name, _)| sensitive.contains(name))
            .flat_map(|(name, value)| header::secret_values(&name, value)),
    );
    found
}

/// The variant, once the parameters it names are known to be acceptable.
pub fn check_srp(config: &Value) -> Result<String, String> {
    crate::srp::SrpParams::from_config(config).map(|params| params.variant.name().to_string())
}

/// What `bddkit resource fields` and `resource add` know about the keys.
/// Kept beside the structs above so a key added to one is not missed in the
/// other.
pub fn fields_json() -> Value {
    json!({
        "api": [
            {"name": "base_url", "required": true,
             "description": "root URL every request is resolved against"},
            {"name": "timeout_secs", "type": "number",
             "description": "per-request timeout in seconds (default 20)"},
            {"name": "default_headers", "type": "nonscalar",
             "description": "headers sent with every request until a scenario replaces them"},
            {"name": "sensitive_headers", "type": "nonscalar",
             "description": "header names masked in failure dumps and reports, added to authorization, cookie, set-cookie"},
            {"name": "sensitive_headers_only", "type": "nonscalar",
             "description": "header names masked instead of authorization, cookie, set-cookie; an empty list masks no header"},
            {"name": "options", "type": "nonscalar",
             "description": "polling.timeout_secs / polling.interval_ms for eventual assertions against this API"}
        ],
        "srp": [
            {"name": "variant", "required": true, "description": "hex-string or rfc5054"},
            {"name": "prime",
             "description": "group prime in hexadecimal (default: the RFC 5054 4096-bit group)"},
            {"name": "generator", "description": "generator as a decimal number (default 5)"},
            {"name": "hash", "description": "sha-1, sha-256 or sha-512 (default sha-256)"},
            {"name": "options", "type": "nonscalar",
             "description": "polling.timeout_secs / polling.interval_ms for eventual assertions using this configuration"}
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(config: &Value) -> Result<Api, String> {
        Api::parse(config)
    }

    #[test]
    fn a_bare_base_url_gets_the_default_timeout_and_the_default_masked_headers() {
        let a = api(&json!({"base_url": "http://x:8080", "colour": "red"})).expect("ok");
        assert_eq!(a.detail(), "http://x:8080 (20s timeout)");
        assert_eq!(
            a.sensitive_headers,
            ["authorization", "cookie", "set-cookie"]
        );
    }

    #[test]
    fn a_base_url_must_be_http_or_https_with_a_host() {
        for bad in ["localhost:8080", "mailto:x@y", "file:///etc", "http://"] {
            assert!(api(&json!({"base_url": bad})).is_err(), "{bad}");
        }
        assert!(api(&json!({"base_url": "https://x"})).is_ok());
    }

    #[test]
    fn the_userinfo_of_a_base_url_is_a_secret_even_when_the_config_is_refused() {
        let config = json!({"base_url": "http://svc:hunter2pass@x", "timeout_secs": 0});
        assert!(api(&config).is_err());
        let found = secrets("api", &config);
        assert!(found.contains(&"hunter2pass".to_string()), "{found:?}");
    }

    #[test]
    fn a_zero_timeout_and_a_bad_url_are_refused() {
        let e = api(&json!({"base_url": "http://x", "timeout_secs": 0})).expect_err("zero");
        assert!(e.contains("timeout_secs is 0"), "{e}");
        let e = api(&json!({"base_url": "not a url"})).expect_err("bad url");
        assert!(e.contains("invalid base_url"), "{e}");
        assert!(api(&json!({})).is_err());
        assert!(api(&json!({"base_url": "http://x", "timeout_secs": -1})).is_err());
    }

    #[test]
    fn default_headers_must_be_a_map_of_strings() {
        assert!(api(&json!({"base_url": "http://x", "default_headers": ["a"]})).is_err());
        assert!(api(&json!({"base_url": "http://x", "default_headers": {"a": 1}})).is_err());
    }

    #[test]
    fn a_header_name_in_default_headers_is_not_checked_here() {
        // A bad name still fails at the request, as it always has.
        api(&json!({"base_url": "http://x", "default_headers": {"bad name": "v"}}))
            .expect("not checked");
    }

    #[test]
    fn sensitive_headers_add_and_the_only_form_replaces() {
        let a =
            api(&json!({"base_url": "http://x", "sensitive_headers": ["X-Api-Key"]})).expect("ok");
        assert_eq!(
            a.sensitive_headers,
            ["authorization", "cookie", "set-cookie", "x-api-key"]
        );
        let a = api(&json!({"base_url": "http://x", "sensitive_headers_only": ["X-Api-Key"]}))
            .expect("ok");
        assert_eq!(a.sensitive_headers, ["x-api-key"]);
        let a = api(&json!({"base_url": "http://x", "sensitive_headers_only": []})).expect("ok");
        assert!(a.sensitive_headers.is_empty());
    }

    #[test]
    fn both_sensitive_lists_or_a_bad_name_are_refused() {
        let e = api(&json!({"base_url": "http://x", "sensitive_headers": [],
                            "sensitive_headers_only": []}))
        .expect_err("both");
        assert!(e.contains("are both set"), "{e}");
        let e = api(&json!({"base_url": "http://x", "sensitive_headers": ["bad name"]}))
            .expect_err("bad");
        assert!(e.contains("not a valid header name"), "{e}");
    }

    #[test]
    fn secrets_are_the_default_headers_under_the_effective_names_whole_and_split() {
        let config = json!({"base_url": "http://x",
            "default_headers": {"Authorization": "Bearer tok-1234567", "X-Trace": "visible"}});
        assert_eq!(
            secrets("api", &config),
            ["Bearer tok-1234567", "tok-1234567"]
        );
        let config = json!({"base_url": "http://x", "sensitive_headers_only": ["x-trace"],
            "default_headers": {"Authorization": "Bearer tok-1234567", "X-Trace": "hidden-value"}});
        assert_eq!(secrets("api", &config), ["hidden-value"]);
    }

    #[test]
    fn secrets_survive_a_config_that_is_refused() {
        let config = json!({"base_url": "nope", "timeout_secs": 0,
            "sensitive_headers": ["x-a"], "sensitive_headers_only": ["x-b"],
            "default_headers": {"Authorization": "Bearer tok-1234567", "x-a": "aaaaaaaa", "x-b": "bbbbbbbb"}});
        assert_eq!(
            secrets("api", &config),
            ["Bearer tok-1234567", "tok-1234567", "aaaaaaaa", "bbbbbbbb"]
        );
        assert!(secrets("srp", &config).is_empty());
    }

    #[test]
    fn an_srp_config_needs_a_known_variant_hash_and_numbers() {
        assert_eq!(
            check_srp(&json!({"variant": "rfc5054"})).as_deref(),
            Ok("rfc5054")
        );
        assert!(check_srp(&json!({})).is_err());
        assert!(check_srp(&json!({"variant": "other"})).is_err());
        assert!(check_srp(&json!({"variant": "hex-string", "hash": "md5"})).is_err());
        assert!(check_srp(&json!({"variant": "hex-string", "prime": "xyz"})).is_err());
        assert!(check_srp(&json!({"variant": "hex-string", "generator": "g"})).is_err());
        check_srp(
            &json!({"variant": "hex-string", "prime": "ff", "generator": "2", "hash": "sha-1"}),
        )
        .expect("ok");
    }

    #[test]
    fn every_key_a_config_reads_is_described() {
        let fields = fields_json();
        let names = |group: &str| -> Vec<String> {
            fields[group]
                .as_array()
                .expect("array")
                .iter()
                .map(|f| f["name"].as_str().expect("name").to_string())
                .collect()
        };
        assert_eq!(
            names("api"),
            [
                "base_url",
                "timeout_secs",
                "default_headers",
                "sensitive_headers",
                "sensitive_headers_only",
                "options"
            ]
        );
        assert_eq!(
            names("srp"),
            ["variant", "prime", "generator", "hash", "options"]
        );
    }
}
