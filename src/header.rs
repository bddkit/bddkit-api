//! Which header values a config or a request holds that must never be printed,
//! and the pieces of them that travel on their own.

use reqwest::header::HeaderName;

/// The headers masked unless `sensitive_headers_only` replaces them.
pub const DEFAULT_SENSITIVE: &[&str] = &["authorization", "cookie", "set-cookie"];

/// A header name in its normal (lowercase) form, or `None` when it is not a
/// valid header name.
pub fn name(value: &str) -> Option<String> {
    HeaderName::from_bytes(value.as_bytes())
        .ok()
        .map(|name| name.as_str().to_string())
}

/// A sensitive header's value whole, and the part of it that travels on its own:
/// the credential of `Authorization: Bearer <credential>`, each value of
/// `Cookie`, the cookie value of `Set-Cookie` (not its attributes). Any other
/// name is registered whole only: splitting it would register more than
/// registering the header ever did.
pub fn secret_values(name: &str, value: &str) -> Vec<String> {
    let mut values = vec![value.to_string()];
    let parts: Vec<&str> = match name.to_ascii_lowercase().as_str() {
        "authorization" | "proxy-authorization" => {
            value.split_once(' ').map(|(_, c)| c).into_iter().collect()
        }
        "cookie" => value
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .map(|(_, v)| v)
            .collect(),
        "set-cookie" => value
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
            .map(|(_, v)| v)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    values.extend(parts.into_iter().map(|part| part.trim().to_string()));
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authorization_value_is_registered_whole_and_without_its_scheme() {
        assert_eq!(
            secret_values("Authorization", "Bearer abc1234"),
            ["Bearer abc1234", "abc1234"]
        );
    }

    #[test]
    fn each_cookie_value_is_registered() {
        assert_eq!(
            secret_values("cookie", "a=one1111; b=two2222"),
            ["a=one1111; b=two2222", "one1111", "two2222"]
        );
    }

    #[test]
    fn a_set_cookie_registers_its_first_pair_only() {
        assert_eq!(
            secret_values("set-cookie", "sid=abc1234; Path=/; HttpOnly"),
            ["sid=abc1234; Path=/; HttpOnly", "abc1234"]
        );
    }

    #[test]
    fn any_other_header_is_registered_whole_only() {
        assert_eq!(secret_values("x-api-key", "k k=v"), ["k k=v"]);
    }

    #[test]
    fn a_name_is_lowercased_and_a_bad_one_refused() {
        assert_eq!(name("X-Api-Key").as_deref(), Some("x-api-key"));
        assert_eq!(name("bad name"), None);
    }
}
