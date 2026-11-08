use super::*;
use serde_json::json;

fn world_with_response(body: &str, headers: Vec<(String, String)>) -> Exchange {
    Exchange {
        method: "GET".to_string(),
        url: "http://x.local/".to_string(),
        request_headers: Vec::new(),
        request_body: None,
        request_form: crate::state::BodyForm::Raw,
        status: 200,
        response_headers: headers,
        response_body: body.to_string(),
        duration_us: 0,
        api: "main".to_string(),
        sensitive_headers: Vec::new(),
    }
}

#[test]
fn contains_text_passes_on_a_substring() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert_eq!(body_contains_text(&w, "<h1>", false), Ok(()));
}

#[test]
fn contains_text_fails_when_absent() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_contains_text(&w, "<h2>", false),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_contains_text_passes_when_absent() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert_eq!(body_contains_text(&w, "<h2>", true), Ok(()));
}

#[test]
fn not_contains_text_fails_when_present() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_contains_text(&w, "<h1>", true),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn matches_passes_on_a_regex_hit() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert_eq!(body_matches(&w, "<h[0-9]>", false), Ok(()));
}

#[test]
fn matches_fails_on_a_regex_miss() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_matches(&w, "<h9>", false),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn matches_is_fatal_on_an_invalid_pattern() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_matches(&w, "[", false),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn not_matches_passes_on_a_regex_miss() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert_eq!(body_matches(&w, "<h9>", true), Ok(()));
}

#[test]
fn not_matches_fails_on_a_regex_hit() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_matches(&w, "<h[0-9]>", true),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_matches_is_fatal_on_an_invalid_pattern_not_swallowed_as_a_pass() {
    let w = world_with_response("<h1>Hello</h1>", Vec::new());
    assert!(matches!(
        body_matches(&w, "[", true),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn not_exists_passes_when_the_path_is_missing() {
    assert_eq!(check_not_exists(&json!({"a": 1}), "b"), Ok(()));
}

#[test]
fn not_exists_fails_when_the_path_is_present() {
    assert!(matches!(
        check_not_exists(&json!({"a": 1}), "a"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_exists_treats_a_null_value_as_present() {
    // Issue #22, item 1: null is not absence.
    assert!(matches!(
        check_not_exists(&json!({"a": null}), "a"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn is_null_passes_on_a_null_node() {
    assert_eq!(check_is_null(&json!({"a": null}), "a"), Ok(()));
}

#[test]
fn is_null_fails_on_a_non_null_node() {
    assert!(matches!(
        check_is_null(&json!({"a": 1}), "a"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn is_null_fails_when_the_node_is_missing() {
    assert!(matches!(
        check_is_null(&json!({"a": 1}), "b"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_null_passes_on_a_non_null_node() {
    assert_eq!(check_not_null(&json!({"a": 1}), "a"), Ok(()));
}

#[test]
fn not_null_fails_on_a_null_node() {
    assert!(matches!(
        check_not_null(&json!({"a": null}), "a"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_null_fails_when_the_node_is_missing() {
    // Existence and non-null are different assertions (issue #22, item 2).
    assert!(matches!(
        check_not_null(&json!({"a": 1}), "b"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_contains_json_passes_when_the_shape_is_absent() {
    assert_eq!(
        check_not_contains_json("body", &json!({"a": 1}), &json!({"b": 2})),
        Ok(())
    );
}

#[test]
fn not_contains_json_fails_when_the_shape_matches() {
    assert!(matches!(
        check_not_contains_json("body", &json!({"a": 1, "b": 2}), &json!({"a": 1})),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_contains_substring_passes_when_the_node_is_missing() {
    assert_eq!(
        check_not_contains_substring(&json!({"a": "hello"}), "b", "ell"),
        Ok(())
    );
}

#[test]
fn not_contains_substring_passes_when_absent() {
    assert_eq!(
        check_not_contains_substring(&json!({"a": "hello"}), "a", "xyz"),
        Ok(())
    );
}

#[test]
fn not_contains_substring_fails_when_present() {
    assert!(matches!(
        check_not_contains_substring(&json!({"a": "hello"}), "a", "ell"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn array_length_on_a_non_array_body_reports_missing_rather_than_a_length_mismatch() {
    // Issue #22, item 5: "not an array" and "wrong length" must stay distinguishable.
    assert_eq!(
        check_array_length(&json!({"a": 1}), 2),
        Err(AttemptError::not_yet("response body is not an array"))
    );
}

#[test]
fn array_length_on_an_array_of_the_wrong_length_reports_the_mismatch() {
    let err = check_array_length(&json!([1, 2]), 3).unwrap_err();
    assert_ne!(
        err,
        AttemptError::not_yet("response body is not an array".to_string())
    );
}

#[test]
fn not_contains_substring_errors_on_a_non_string_node() {
    assert!(matches!(
        check_not_contains_substring(&json!({"a": 1}), "a", "1"),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn has_element_passes_on_html() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(
        r#"<html><body><h1 id="title">Hi</h1></body></html>"#,
        headers,
    );
    assert_eq!(body_has_element(&w, "h1#title"), Ok(()));
}

#[test]
fn has_element_passes_on_xml() {
    let headers = vec![("content-type".to_string(), "application/xml".to_string())];
    let w = world_with_response(r"<root><name>Acme</name></root>", headers);
    assert_eq!(body_has_element(&w, "//name"), Ok(()));
}

#[test]
fn has_element_fails_when_no_match() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(r"<html><body></body></html>", headers);
    assert!(matches!(
        body_has_element(&w, ".missing"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn has_element_is_fatal_on_json_content_type() {
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let w = world_with_response(r#"{"a": 1}"#, headers);
    assert!(matches!(
        body_has_element(&w, "a"),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn has_element_with_text_passes_on_exact_match() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(
        r#"<html><body><h1 id="title">Hi</h1></body></html>"#,
        headers,
    );
    assert_eq!(body_has_element_with_text(&w, "h1#title", "Hi"), Ok(()));
}

#[test]
fn has_element_with_text_fails_on_mismatch() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(
        r#"<html><body><h1 id="title">Hi</h1></body></html>"#,
        headers,
    );
    assert!(matches!(
        body_has_element_with_text(&w, "h1#title", "Bye"),
        Err(AttemptError::NotYet(_))
    ));
}

#[test]
fn not_has_element_passes_when_absent() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(r"<html><body></body></html>", headers);
    assert_eq!(body_not_has_element(&w, ".missing"), Ok(()));
}

#[test]
fn not_has_element_fails_when_present() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let w = world_with_response(
        r#"<html><body><h1 id="title">Hi</h1></body></html>"#,
        headers,
    );
    assert!(matches!(
        body_not_has_element(&w, "h1#title"),
        Err(AttemptError::NotYet(_))
    ));
}

/// Negation must not swallow a config-level error: "wrong content-type"
/// is a failure to evaluate, not a successful absence.
#[test]
fn not_has_element_stays_fatal_on_json_content_type() {
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let w = world_with_response(r#"{"a": 1}"#, headers);
    assert!(matches!(
        body_not_has_element(&w, "a"),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn has_element_does_not_silently_pass_on_a_false_boolean_xpath() {
    let headers = vec![("content-type".to_string(), "application/xml".to_string())];
    let w = world_with_response(r"<root><name>Acme</name></root>", headers);
    assert!(matches!(
        body_has_element(&w, "//missing = 'x'"),
        Err(AttemptError::Fatal(..))
    ));
}

#[test]
fn has_element_with_text_fails_on_whitespace_only_difference_and_the_message_shows_it() {
    let headers = vec![("content-type".to_string(), "text/html".to_string())];
    let indented = "<html><body>\n  <div class=\"box\">\n    Loose\n  </div>\n</body></html>";
    let w = world_with_response(indented, headers);
    let err = body_has_element_with_text(&w, "div.box", "Loose").unwrap_err();
    let AttemptError::NotYet(msg) = err else {
        panic!("expected NotYet")
    };
    // Debug-quoted so the whitespace difference is visible, not silently swallowed.
    assert!(
        msg.contains("\\n"),
        "message should show escaped whitespace: {msg}"
    );
}

fn hdrs(pairs: &[(&str, &str)]) -> Exchange {
    let headers = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    world_with_response("", headers)
}

fn not_yet<T>(r: &Result<T, AttemptError>) -> bool {
    matches!(r, Err(AttemptError::NotYet(_)))
}

#[test]
fn header_is_finds_a_value_on_a_later_line() {
    let w = hdrs(&[("Vary", "Accept"), ("vary", "Accept-Encoding")]);
    assert_eq!(response_header(&w, "VARY", "Accept-Encoding"), Ok(()));
    assert!(not_yet(&response_header(&w, "Vary", "Origin")));
}

#[test]
fn header_contains_passes_fails_and_reports_missing() {
    let w = hdrs(&[("Cache-Control", "public, max-age=3600")]);
    assert_eq!(
        response_header_contains(&w, "cache-control", "max-age", false),
        Ok(())
    );
    assert!(not_yet(&response_header_contains(
        &w,
        "Cache-Control",
        "private",
        false
    )));
    let Err(AttemptError::NotYet(e)) = response_header_contains(&w, "Link", "x", false) else {
        panic!("expected NotYet")
    };
    assert!(e.contains("response header \"Link\" is missing"));
}

#[test]
fn header_contains_matches_on_the_second_line_only() {
    let w = hdrs(&[("Link", "</a>; rel=\"x\""), ("Link", "</b>; rel=\"y\"")]);
    assert_eq!(response_header_contains(&w, "Link", "</b>", false), Ok(()));
    let Err(AttemptError::NotYet(e)) = response_header_contains(&w, "Link", "</c>", false) else {
        panic!("expected NotYet")
    };
    assert!(e.contains("</a>") && e.contains("</b>"));
}

#[test]
fn header_not_contains_needs_every_line_clean_and_passes_when_missing() {
    let w = hdrs(&[("Vary", "Accept"), ("Vary", "Cookie")]);
    assert_eq!(response_header_contains(&w, "Vary", "Origin", true), Ok(()));
    assert!(not_yet(&response_header_contains(
        &w, "Vary", "Cookie", true
    )));
    assert_eq!(response_header_contains(&w, "Link", "x", true), Ok(()));
}

#[test]
fn header_matches_and_not_matches() {
    let w = hdrs(&[
        ("Set-Cookie", "a=1"),
        ("Set-Cookie", "session=abc; HttpOnly"),
    ]);
    assert_eq!(
        response_header_matches(&w, "set-cookie", r"^session=\w+; HttpOnly$", false),
        Ok(())
    );
    assert!(not_yet(&response_header_matches(
        &w,
        "Set-Cookie",
        "^zzz",
        false
    )));
    assert!(not_yet(&response_header_matches(
        &w, "Location", ".", false
    )));
    assert_eq!(
        response_header_matches(&w, "Set-Cookie", "^zzz", true),
        Ok(())
    );
    assert!(not_yet(&response_header_matches(
        &w,
        "Set-Cookie",
        "^a=",
        true
    )));
    assert_eq!(response_header_matches(&w, "Location", ".", true), Ok(()));
}

#[test]
fn header_matches_with_an_invalid_regex_is_fatal() {
    let w = hdrs(&[("Vary", "Accept")]);
    for negate in [false, true] {
        assert!(matches!(
            response_header_matches(&w, "Vary", "[", negate),
            Err(AttemptError::Fatal(..))
        ));
        assert!(matches!(
            response_header_matches(&w, "Missing", "[", negate),
            Err(AttemptError::Fatal(..))
        ));
    }
}
