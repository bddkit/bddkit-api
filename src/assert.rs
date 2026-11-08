//! The response assertions, ported from the host's built-in API steps. Each
//! check reads one `Exchange`; getting a fresh one (attempt 2 onwards) is the
//! caller's business, see `steps::observe`.

use bddkit_crate_json::{matcher, path};
use regex::Regex;
use serde_json::Value;

use crate::markup::{self, MarkupError};
use crate::reply::Class;
use crate::state::Exchange;

/// A check that did not hold. `NotYet` may hold on a later attempt (class
/// `assertion`); `Fatal` cannot change before the next attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum AttemptError {
    NotYet(String),
    Fatal(Class, String),
}

pub type AttemptResult = Result<(), AttemptError>;

impl AttemptError {
    pub fn not_yet(message: impl Into<String>) -> Self {
        Self::NotYet(message.into())
    }

    /// An assertion that cannot change before the next attempt.
    pub fn assertion(message: impl Into<String>) -> Self {
        Self::Fatal(Class::Assertion, message.into())
    }

    /// The step itself is wrong: no attempt can fix it.
    pub fn input(message: impl Into<String>) -> Self {
        Self::Fatal(Class::Input, message.into())
    }
}

pub fn response_code(ex: &Exchange, expected: &str) -> AttemptResult {
    let want: u16 = expected
        .parse()
        .map_err(|_| AttemptError::input(format!("invalid code {expected:?}")))?;
    let got = ex.status;
    if got == want {
        Ok(())
    } else {
        Err(AttemptError::not_yet(format!(
            "    expected: {want}\n    actual:   {got}"
        )))
    }
}

/// Every value of a response header, one per line the server sent, matched by
/// name case-insensitively.
fn header_values<'a>(ex: &'a Exchange, name: &str) -> Vec<&'a str> {
    ex.response_headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .collect()
}

pub fn response_header(ex: &Exchange, name: &str, value: &str) -> AttemptResult {
    let got = header_values(ex, name);
    if got.contains(&value) {
        return Ok(());
    }
    if got.is_empty() {
        return Err(AttemptError::not_yet(format!(
            "response header {name:?} is missing"
        )));
    }
    Err(AttemptError::not_yet(format!(
        "    expected: {value}\n    actual:   {}",
        got.join("\n              ")
    )))
}

/// A text check over every line of a response header. Positive: some line
/// passes, and a missing header fails. Negative: no line fails, and a missing
/// header passes. `check` is `check_contains`/`check_matches` for one line.
fn header_text(
    ex: &Exchange,
    name: &str,
    negate: bool,
    check: impl Fn(&str, &str, bool) -> AttemptResult,
) -> AttemptResult {
    let values = header_values(ex, name);
    if values.is_empty() {
        return if negate {
            Ok(())
        } else {
            Err(AttemptError::not_yet(format!(
                "response header {name:?} is missing"
            )))
        };
    }
    let subject = format!("response header {name:?}");
    let mut first_failure = None;
    for value in &values {
        match check(&subject, value, negate) {
            Ok(()) if !negate => return Ok(()),
            Ok(()) => {}
            Err(failure) => {
                first_failure.get_or_insert(failure);
                if negate {
                    break;
                }
            }
        }
    }
    match first_failure {
        // The check failed: the first failing line is the message, with every
        // line of the header added when the server sent more than one.
        Some(AttemptError::NotYet(mut message)) if values.len() > 1 => {
            message.push_str(&format!(
                "\n    all {} lines of {name:?}:\n{}",
                values.len(),
                values.join("\n")
            ));
            Err(AttemptError::NotYet(message))
        }
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

pub fn response_header_contains(
    ex: &Exchange,
    name: &str,
    needle: &str,
    negate: bool,
) -> AttemptResult {
    header_text(ex, name, negate, |subject, text, negate| {
        check_contains(subject, text, needle, negate)
    })
}

pub fn response_header_matches(
    ex: &Exchange,
    name: &str,
    pattern: &str,
    negate: bool,
) -> AttemptResult {
    // A bad pattern is fatal even when the header is missing.
    Regex::new(pattern)
        .map_err(|e| AttemptError::input(format!("invalid regex {pattern:?}: {e}")))?;
    header_text(ex, name, negate, |subject, text, negate| {
        check_matches(subject, text, pattern, negate)
    })
}

fn body_json(ex: &Exchange) -> Result<Value, AttemptError> {
    serde_json::from_str(&ex.response_body)
        .map_err(|e| AttemptError::not_yet(format!("response body is not valid JSON: {e}")))
}

/// The doc string of a JSON step, parsed.
fn expected_json(docstring: Option<&str>) -> Result<Value, AttemptError> {
    let source =
        docstring.ok_or_else(|| AttemptError::input("step requires a doc string with JSON"))?;
    bddkit_crate_json::parse_expected(source).map_err(|e| AttemptError::input(e.to_string()))
}

pub fn body_contains_json(ex: &Exchange, docstring: Option<&str>) -> AttemptResult {
    let expected = expected_json(docstring)?;
    matcher::contains(&body_json(ex)?, &expected)
        .map_err(|mismatch| AttemptError::not_yet(mismatch.to_string()))
}

pub fn body_equals_json(ex: &Exchange, docstring: Option<&str>) -> AttemptResult {
    let expected = expected_json(docstring)?;
    matcher::equals(&body_json(ex)?, &expected)
        .map_err(|mismatch| AttemptError::not_yet(mismatch.to_string()))
}

pub fn body_not_contains_json(ex: &Exchange, docstring: Option<&str>) -> AttemptResult {
    let expected = expected_json(docstring)?;
    check_not_contains_json("body", &body_json(ex)?, &expected)
}

fn check_not_contains_json(subject: &str, actual: &Value, expected: &Value) -> AttemptResult {
    matcher::not_contains(actual, expected)
        .map_err(|unexpected| AttemptError::not_yet(unexpected.describe(subject)))
}

pub fn array_length(ex: &Exchange, expected: &str) -> AttemptResult {
    let want: usize = expected
        .parse()
        .map_err(|_| AttemptError::input(format!("invalid length {expected:?}")))?;
    check_array_length(&body_json(ex)?, want)
}

/// "body is not an array at all" and "array has the wrong length" are
/// distinguishable failures — two messages, not a new step.
fn check_array_length(v: &Value, want: usize) -> AttemptResult {
    let arr = v
        .as_array()
        .ok_or_else(|| AttemptError::not_yet("response body is not an array"))?;
    if arr.len() == want {
        Ok(())
    } else {
        Err(AttemptError::not_yet(format!(
            "    expected: array of length {want}\n    actual:   length {}",
            arr.len()
        )))
    }
}

/// The body parsed as JSON, once the path is known to be well formed.
fn node_subject(ex: &Exchange, p: &str) -> Result<Value, AttemptError> {
    path::validate(p).map_err(|e| AttemptError::input(e.to_string()))?;
    body_json(ex)
}

pub fn json_node_exists(ex: &Exchange, p: &str) -> AttemptResult {
    path::read(&node_subject(ex, p)?, p)
        .map(|_| ())
        .map_err(|e| AttemptError::not_yet(e.to_string()))
}

pub fn json_node_not_exists(ex: &Exchange, p: &str) -> AttemptResult {
    check_not_exists(&node_subject(ex, p)?, p)
}

/// A `null` node exists — this must not treat `null` as absent.
fn check_not_exists(v: &Value, p: &str) -> AttemptResult {
    match path::read(v, p) {
        Err(_) => Ok(()),
        Ok(node) => Err(AttemptError::not_yet(format!(
            "    expected: node {p:?} to not exist\n    actual:   {node}"
        ))),
    }
}

pub fn json_node_is_null(ex: &Exchange, p: &str) -> AttemptResult {
    check_is_null(&node_subject(ex, p)?, p)
}

pub fn json_node_not_null(ex: &Exchange, p: &str) -> AttemptResult {
    check_not_null(&node_subject(ex, p)?, p)
}

/// "does not exist" and "exists but not null" are different bugs — a missing
/// node is always its own message, never folded into "not null".
fn check_is_null(v: &Value, p: &str) -> AttemptResult {
    match path::read(v, p) {
        Err(_) => Err(AttemptError::not_yet(format!("node {p:?} does not exist"))),
        Ok(node) if node.is_null() => Ok(()),
        Ok(node) => Err(AttemptError::not_yet(format!(
            "    expected: null\n    actual:   {node}"
        ))),
    }
}

fn check_not_null(v: &Value, p: &str) -> AttemptResult {
    match path::read(v, p) {
        Err(_) => Err(AttemptError::not_yet(format!("node {p:?} does not exist"))),
        Ok(node) if node.is_null() => Err(AttemptError::not_yet(
            "    expected: not null\n    actual:   null",
        )),
        Ok(_) => Ok(()),
    }
}

pub fn json_node_not_contains(ex: &Exchange, p: &str, needle: &str) -> AttemptResult {
    check_not_contains_substring(&node_subject(ex, p)?, p, needle)
}

/// A node that isn't a string is a type bug, not a pass — never silently
/// stringified. A missing node has nothing to contain the substring, so it
/// passes.
fn check_not_contains_substring(v: &Value, p: &str, needle: &str) -> AttemptResult {
    match path::read(v, p) {
        Ok(Value::String(s)) if s.contains(needle) => Err(AttemptError::not_yet(format!(
            "    node {p:?} should not contain {needle:?}\n    actual:   {s:?}"
        ))),
        Err(_) | Ok(Value::String(_)) => Ok(()),
        Ok(other) => Err(AttemptError::assertion(format!(
            "node {p:?} is not a string, got: {other}"
        ))),
    }
}

/// The raw-text checks, over the response body or one line of a header.
fn check_empty(subject: &str, text: &str) -> AttemptResult {
    if text.trim().is_empty() {
        Ok(())
    } else {
        Err(AttemptError::not_yet(format!(
            "    expected: empty {subject}\n    actual:   {text:?}"
        )))
    }
}

fn check_contains(subject: &str, text: &str, needle: &str, negate: bool) -> AttemptResult {
    if text.contains(needle) != negate {
        return Ok(());
    }
    let not = if negate { "NOT " } else { "" };
    Err(AttemptError::not_yet(format!(
        "    expected {subject} to {not}contain: {needle:?}\n    actual {subject}:\n{text}"
    )))
}

fn check_matches(subject: &str, text: &str, pattern: &str, negate: bool) -> AttemptResult {
    let re = Regex::new(pattern)
        .map_err(|e| AttemptError::input(format!("invalid regex {pattern:?}: {e}")))?;
    if re.is_match(text) != negate {
        return Ok(());
    }
    let not = if negate { "NOT " } else { "" };
    Err(AttemptError::not_yet(format!(
        "    expected {subject} to {not}match: {pattern}\n    actual {subject}:\n{text}"
    )))
}

pub fn body_empty(ex: &Exchange) -> AttemptResult {
    check_empty("body", &ex.response_body)
}

pub fn body_contains_text(ex: &Exchange, needle: &str, negate: bool) -> AttemptResult {
    check_contains("body", &ex.response_body, needle, negate)
}

pub fn body_matches(ex: &Exchange, pattern: &str, negate: bool) -> AttemptResult {
    check_matches("body", &ex.response_body, pattern, negate)
}

fn selected_element(ex: &Exchange, selector: &str) -> Result<String, AttemptError> {
    let kind = markup::classify(markup::content_type(&ex.response_headers));
    markup::select(kind, &ex.response_body, selector, true).map_err(|e| match e {
        MarkupError::NoMatch(msg) => AttemptError::not_yet(msg),
        MarkupError::Fatal(msg) => AttemptError::input(msg),
    })
}

pub fn body_has_element(ex: &Exchange, selector: &str) -> AttemptResult {
    selected_element(ex, selector)?;
    Ok(())
}

pub fn body_has_element_with_text(ex: &Exchange, selector: &str, text: &str) -> AttemptResult {
    let got = selected_element(ex, selector)?;
    if got == text {
        Ok(())
    } else {
        Err(AttemptError::not_yet(format!(
            "    expected: {text:?}\n    actual:   {got:?}"
        )))
    }
}

pub fn body_not_has_element(ex: &Exchange, selector: &str) -> AttemptResult {
    match selected_element(ex, selector) {
        Ok(got) => Err(AttemptError::not_yet(format!(
            "    expected: no element to match {selector:?}\n    actual:   matched {got:?}"
        ))),
        Err(AttemptError::NotYet(_)) => Ok(()),
        Err(fatal) => Err(fatal),
    }
}

#[cfg(test)]
#[path = "assert_tests.rs"]
mod tests;
