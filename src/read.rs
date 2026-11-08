//! The steps that read the last response: `extract` into a variable and
//! `Print response …` to the host's stderr. They are not polled and write no
//! request state; a failure is classed where it is born, as the host's own
//! versions of these steps did.

use bddkit_crate_json::path;
use serde_json::{Value, json};

use crate::markup::{self, BodyKind, MarkupError};
use crate::reply::{Class, Reply};
use crate::state::Exchange;

/// What a read step that cannot be satisfied says: the class and the message.
pub type Failure = (Class, String);

fn input(message: impl Into<String>) -> Failure {
    (Class::Input, message.into())
}

fn assertion(message: impl Into<String>) -> Failure {
    (Class::Assertion, message.into())
}

/// A scalar is stored as it is, a string without its JSON quotes.
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn body_json(ex: &Exchange) -> Result<Value, String> {
    serde_json::from_str(&ex.response_body)
        .map_err(|e| format!("response body is not valid JSON: {e}"))
}

fn body_kind(ex: &Exchange) -> BodyKind {
    markup::classify(markup::content_type(&ex.response_headers))
}

pub fn extract_json(ex: &Exchange, p: &str) -> Result<String, Failure> {
    let root = body_json(ex).map_err(assertion)?;
    path::read(&root, p)
        .map(scalar)
        .map_err(|e| assertion(e.to_string()))
}

/// The value of the first `Set-Cookie` line that sets this cookie.
pub fn extract_cookie(ex: &Exchange, name: &str) -> Result<String, Failure> {
    ex.response_headers
        .iter()
        .filter(|(header, _)| header.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, line)| line.split(';').next()?.split_once('='))
        .find(|(cookie, _)| cookie.trim() == name)
        .map(|(_, value)| value.trim().to_string())
        .ok_or_else(|| assertion(format!("cookie {name:?} not found in the response")))
}

/// An XPath may select a computed value here, not only a node-set.
pub fn select(ex: &Exchange, selector: &str) -> Result<String, Failure> {
    markup::select(body_kind(ex), &ex.response_body, selector, false).map_err(|e| match e {
        MarkupError::NoMatch(message) => assertion(message),
        MarkupError::Fatal(message) => input(message),
    })
}

/// The values go to the host as they are; masking them is the host's business,
/// and `--reveal` must be able to show them.
pub fn print_headers(ex: &Exchange) -> Value {
    json!({
        "title": "=== Response headers ===",
        "headers": ex.response_headers,
        "sensitive_headers": ex.sensitive_headers,
    })
}

fn text(text: &str, syntax: Option<&str>) -> Value {
    json!({"text": text, "syntax": syntax})
}

pub fn print_body(ex: &Exchange) -> Result<Value, Failure> {
    Ok(match body_kind(ex) {
        BodyKind::Json => {
            // The host pretty-prints without checking; the core refused a body
            // that is not JSON under a JSON content type, and so does this.
            body_json(ex).map_err(input)?;
            text(&ex.response_body, Some("json"))
        }
        BodyKind::Xml => text(&ex.response_body, Some("xml")),
        BodyKind::Html => text(&ex.response_body, Some("html")),
        BodyKind::Plain => text(&ex.response_body, None),
    })
}

pub fn print_body_as(ex: &Exchange, selector: &str) -> Result<Value, Failure> {
    match body_kind(ex) {
        BodyKind::Json => {
            let root = body_json(ex).map_err(input)?;
            let node = path::read(&root, selector).map_err(|e| input(e.to_string()))?;
            let pretty = serde_json::to_string_pretty(node)
                .map_err(|e| input(format!("failed to serialize JSON: {e}")))?;
            Ok(text(&pretty, Some("json")))
        }
        BodyKind::Xml | BodyKind::Html => select(ex, selector)
            .map(|selected| text(&selected, None))
            .map_err(|(_, message)| input(message)),
        BodyKind::Plain => Err(input(
            "path selection is not supported: content-type is not JSON/XML/HTML",
        )),
    }
}

/// A step that reads the response and writes a variable named by its `name`.
pub fn extract_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "action",
        "description": description, "reads": ["response"], "sets_vars": ["{name}"],
    })
}

/// A step that reads the response and writes nothing.
pub fn print_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "action",
        "description": description, "reads": ["response"], "sets_vars": [],
    })
}

/// The reply of a step that read a value into a variable.
pub fn stored(name: &str, value: String, global: bool) -> Reply {
    let mut reply = Reply::passed();
    reply.set_var(name, value, global);
    reply
}

/// The reply of a step that printed.
pub fn printed(item: Value) -> Reply {
    let mut reply = Reply::passed();
    reply.print(item);
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BodyForm;

    fn exchange(content_type: &str, body: &str, extra: &[(&str, &str)]) -> Exchange {
        let mut headers = vec![("Content-Type".to_string(), content_type.to_string())];
        headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        Exchange {
            method: "GET".into(),
            url: "http://x/".into(),
            request_headers: Vec::new(),
            request_body: None,
            request_form: BodyForm::Raw,
            status: 200,
            response_headers: headers,
            response_body: body.into(),
            duration_us: 0,
            api: "main".into(),
            sensitive_headers: vec!["set-cookie".into()],
        }
    }

    #[test]
    fn a_json_scalar_is_stored_without_quotes_and_the_rest_as_json() {
        let ex = exchange("application/json", r#"{"a":"x","n":3,"o":{"k":1}}"#, &[]);
        assert_eq!(extract_json(&ex, "a").as_deref(), Ok("x"));
        assert_eq!(extract_json(&ex, "n").as_deref(), Ok("3"));
        assert_eq!(extract_json(&ex, "o").as_deref(), Ok(r#"{"k":1}"#));
    }

    #[test]
    fn a_missing_node_or_a_body_that_is_not_json_is_an_assertion() {
        let ex = exchange("application/json", r#"{"a":1}"#, &[]);
        assert_eq!(extract_json(&ex, "b").unwrap_err().0, Class::Assertion);
        let ex = exchange("application/json", "nope", &[]);
        assert_eq!(extract_json(&ex, "a").unwrap_err().0, Class::Assertion);
    }

    #[test]
    fn a_cookie_is_found_by_name_across_lines_and_header_case() {
        let ex = exchange(
            "text/plain",
            "",
            &[
                ("set-cookie", "other=1; Path=/"),
                ("Set-Cookie", "jwt_token = tok-abc ; HttpOnly"),
            ],
        );
        assert_eq!(extract_cookie(&ex, "jwt_token").as_deref(), Ok("tok-abc"));
        assert_eq!(extract_cookie(&ex, "jwt").unwrap_err().0, Class::Assertion);
    }

    #[test]
    fn an_xpath_may_select_an_attribute_and_a_bad_content_type_is_input() {
        let ex = exchange(
            "application/xml",
            r#"<u><user id="1"/><user id="2"/></u>"#,
            &[],
        );
        assert_eq!(select(&ex, "//user[1]/@id").as_deref(), Ok("1"));
        let ex = exchange("application/json", "{}", &[]);
        assert_eq!(select(&ex, "p").unwrap_err().0, Class::Input);
    }

    #[test]
    fn the_headers_item_carries_raw_values_and_the_exchanges_sensitive_names() {
        let ex = exchange("text/plain", "", &[("Set-Cookie", "jwt=tok-abc")]);
        let item = print_headers(&ex);
        assert_eq!(item["title"], "=== Response headers ===");
        assert!(item["headers"].to_string().contains("tok-abc"));
        assert_eq!(item["sensitive_headers"], json!(["set-cookie"]));
    }

    #[test]
    fn a_body_prints_with_the_syntax_of_its_content_type() {
        for (ct, syntax) in [
            ("application/json", json!("json")),
            ("application/xml", json!("xml")),
            ("text/html", json!("html")),
            ("text/plain", Value::Null),
        ] {
            let body = if ct.ends_with("json") { "{}" } else { "<a/>" };
            let item = print_body(&exchange(ct, body, &[])).expect("prints");
            assert_eq!(item["syntax"], syntax, "{ct}");
            assert_eq!(item["text"], body);
        }
        let broken = exchange("application/json", "nope", &[]);
        assert_eq!(print_body(&broken).unwrap_err().0, Class::Input);
    }

    #[test]
    fn a_selected_path_prints_as_json_and_a_selected_element_as_plain_text() {
        let ex = exchange("application/json", r#"{"a":{"b":1}}"#, &[]);
        let item = print_body_as(&ex, "a").expect("prints");
        assert_eq!(item["syntax"], "json");
        assert!(
            item["text"]
                .as_str()
                .is_some_and(|t| t.contains("\"b\": 1"))
        );
        let ex = exchange("text/html", "<p>hi</p>", &[]);
        let item = print_body_as(&ex, "p").expect("prints");
        assert_eq!(
            (item["text"].as_str(), item["syntax"].is_null()),
            (Some("hi"), true)
        );
        let ex = exchange("text/plain", "x", &[]);
        assert_eq!(print_body_as(&ex, "p").unwrap_err().0, Class::Input);
    }
}
