//! The steps this build declares and what the ones that build and send a
//! request do. The index in `ALL` is the step's identity at dispatch, so the
//! array and `Step::from_index` live together.

use serde_json::{Value, json};

use crate::assert::{self, AttemptError, AttemptResult};
use crate::header;
use crate::login;
use crate::read;
use crate::reply::{Class, Reply};
use crate::send;
use crate::state::{Exchange, State};
use crate::upload;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Request,
    ResponseCode,
    RequestUsing,
    SetHeader,
    AddHeader,
    SetQuery,
    SetBody,
    EmptyBody,
    SetForm,
    Attach,
    Hawk,
    ResponseBodyContainsJson,
    ResponseBodyEqualsJson,
    ResponseArrayLength,
    ResponseHeader,
    ResponseHeaderContains,
    ResponseHeaderNotContains,
    ResponseHeaderMatches,
    ResponseHeaderNotMatches,
    JsonNodeExists,
    JsonNodeNotExists,
    JsonNodeIsNull,
    JsonNodeNotNull,
    ResponseBodyNotContainsJson,
    JsonNodeNotContainsSubstring,
    ResponseBodyEmpty,
    ResponseBodyContains,
    ResponseBodyNotContains,
    ResponseBodyMatches,
    ResponseBodyNotMatches,
    ResponseBodyHasElement,
    ResponseBodyHasElementWithText,
    ResponseBodyNotHasElement,
    ExtractJson,
    ExtractJsonGlobal,
    ExtractCookie,
    ExtractCookieGlobal,
    ExtractMarkup,
    PrintHeaders,
    PrintBody,
    PrintBodyAs,
    SrpVerifier,
    SrpVerifierWithSalt,
    SrpStartLogin,
    SrpCompleteLogin,
}

const ALL: [Step; 45] = [
    Step::Request,
    Step::ResponseCode,
    Step::RequestUsing,
    Step::SetHeader,
    Step::AddHeader,
    Step::SetQuery,
    Step::SetBody,
    Step::EmptyBody,
    Step::SetForm,
    Step::Attach,
    Step::Hawk,
    Step::ResponseBodyContainsJson,
    Step::ResponseBodyEqualsJson,
    Step::ResponseArrayLength,
    Step::ResponseHeader,
    Step::ResponseHeaderContains,
    Step::ResponseHeaderNotContains,
    Step::ResponseHeaderMatches,
    Step::ResponseHeaderNotMatches,
    Step::JsonNodeExists,
    Step::JsonNodeNotExists,
    Step::JsonNodeIsNull,
    Step::JsonNodeNotNull,
    Step::ResponseBodyNotContainsJson,
    Step::JsonNodeNotContainsSubstring,
    Step::ResponseBodyEmpty,
    Step::ResponseBodyContains,
    Step::ResponseBodyNotContains,
    Step::ResponseBodyMatches,
    Step::ResponseBodyNotMatches,
    Step::ResponseBodyHasElement,
    Step::ResponseBodyHasElementWithText,
    Step::ResponseBodyNotHasElement,
    Step::ExtractJson,
    Step::ExtractJsonGlobal,
    Step::ExtractCookie,
    Step::ExtractCookieGlobal,
    Step::ExtractMarkup,
    Step::PrintHeaders,
    Step::PrintBody,
    Step::PrintBodyAs,
    Step::SrpVerifier,
    Step::SrpVerifierWithSalt,
    Step::SrpStartLogin,
    Step::SrpCompleteLogin,
];

impl Step {
    pub fn from_index(index: u32) -> Option<Self> {
        ALL.get(usize::try_from(index).ok()?).copied()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per step; a table would hide the text"
    )]
    fn spec(self) -> Value {
        match self {
            Step::Request => send_spec(
                r#"I request "{path}""#,
                "sends a GET request to the path, resolved against the API's base URL",
            ),
            Step::RequestUsing => send_spec(
                r#"I request "{path}" using HTTP {method:method}"#,
                "sends a request with this method to the path, resolved against the API's base URL",
            ),
            Step::ResponseCode => assert_spec(
                "the response code is {code:uint}",
                "asserts the HTTP status code of the last response",
            ),
            Step::ResponseBodyContainsJson => assert_spec(
                "the response body contains JSON:",
                "asserts the response body contains this JSON — object subsets pass, array order is ignored",
            ),
            Step::ResponseBodyEqualsJson => assert_spec(
                "the response body equals JSON:",
                "asserts the response body equals this JSON exactly, key for key and element for element",
            ),
            Step::ResponseArrayLength => assert_spec(
                "the response body is a JSON array of length {length:uint}",
                "asserts the response body is a JSON array holding this many elements",
            ),
            Step::ResponseHeader => assert_spec(
                r#"the "{name}" response header is "{value}""#,
                "asserts a response header holds exactly this value — on any line, when the server sent the header more than once",
            ),
            Step::ResponseHeaderContains => assert_spec(
                r#"the "{name}" response header contains "{text:any}""#,
                "asserts a response header's value contains this text — on any line, when the header repeats",
            ),
            Step::ResponseHeaderNotContains => assert_spec(
                r#"the "{name}" response header does not contain "{text:any}""#,
                "asserts no line of a response header contains this text — passes when the header is absent",
            ),
            Step::ResponseHeaderMatches => assert_spec(
                r#"the "{name}" response header matches "{pattern:any}""#,
                "asserts a response header's value matches this regular expression — on any line, when the header repeats",
            ),
            Step::ResponseHeaderNotMatches => assert_spec(
                r#"the "{name}" response header does not match "{pattern:any}""#,
                "asserts no line of a response header matches this regular expression — passes when the header is absent",
            ),
            Step::JsonNodeExists => assert_spec(
                r#"the JSON node "{path}" should exist"#,
                "asserts the response body has a node at this JSON path",
            ),
            Step::JsonNodeNotExists => assert_spec(
                r#"the JSON node "{path}" should not exist"#,
                "asserts the response body has no node at this JSON path — a null value still counts as existing",
            ),
            Step::JsonNodeIsNull => assert_spec(
                r#"the JSON node "{path}" should be null"#,
                "asserts the node at this JSON path exists and is JSON null",
            ),
            Step::JsonNodeNotNull => assert_spec(
                r#"the JSON node "{path}" should not be null"#,
                "asserts the node at this JSON path exists and is not JSON null",
            ),
            Step::ResponseBodyNotContainsJson => assert_spec(
                "the response body does not contain JSON:",
                "asserts the response body does not contain this JSON — same subset/order-independent matching as `contains`, inverted",
            ),
            Step::JsonNodeNotContainsSubstring => assert_spec(
                r#"the JSON node "{path}" should not contain "{substring}""#,
                "asserts a string node does not contain this substring",
            ),
            Step::ResponseBodyEmpty => assert_spec(
                "the response body should be empty",
                "asserts the response body is empty",
            ),
            Step::ResponseBodyContains => assert_spec(
                r#"the response body contains "{text}""#,
                "asserts the raw response body contains this text — works for any content type",
            ),
            Step::ResponseBodyNotContains => assert_spec(
                r#"the response body does not contain "{text}""#,
                "asserts the raw response body does not contain this text",
            ),
            Step::ResponseBodyMatches => assert_spec(
                r#"the response body matches "{pattern}""#,
                "asserts the raw response body matches this regular expression",
            ),
            Step::ResponseBodyNotMatches => assert_spec(
                r#"the response body does not match "{pattern}""#,
                "asserts the raw response body does not match this regular expression",
            ),
            Step::ResponseBodyHasElement => assert_spec(
                r#"the response body has element "{selector}""#,
                "asserts an HTML (CSS selector) or XML (XPath) element matches in the response body",
            ),
            Step::ResponseBodyHasElementWithText => assert_spec(
                r#"the response body has element "{selector}" with text "{text}""#,
                "asserts the matched elements' text, newline-joined if more than one, equals this value exactly",
            ),
            Step::ResponseBodyNotHasElement => assert_spec(
                r#"the response body does not have element "{selector}""#,
                "asserts no element matches this selector in the response body",
            ),
            Step::ExtractJson => read::extract_spec(
                r#"extract "{path}" from JSON as "{name}""#,
                "reads a JSON path of the response into a variable",
            ),
            Step::ExtractJsonGlobal => read::extract_spec(
                r#"extract "{path}" from JSON as "{name}" global"#,
                "reads a JSON path of the response into a variable the whole file can see",
            ),
            Step::ExtractCookie => read::extract_spec(
                r#"extract "{cookie}" from cookies as "{name}""#,
                "reads a response cookie into a variable",
            ),
            Step::ExtractCookieGlobal => read::extract_spec(
                r#"extract "{cookie}" from cookies as "{name}" global"#,
                "reads a response cookie into a variable the whole file can see",
            ),
            Step::ExtractMarkup => read::extract_spec(
                r#"extract "{selector}" from response body as "{name}""#,
                "reads the matched elements' text, newline-joined if more than one, into a variable",
            ),
            Step::PrintHeaders => read::print_spec(
                "Print response headers",
                "prints the headers of the last response to stderr",
            ),
            Step::PrintBody => read::print_spec(
                "Print response body",
                "prints the body of the last response to stderr, highlighted",
            ),
            Step::PrintBodyAs => read::print_spec(
                r#"Print response body as "{path}""#,
                "prints one JSON path, XPath (XML) or CSS selector (HTML) selection of the last response to stderr",
            ),
            Step::SetHeader => build_spec(
                r#"the "{name}" request header is "{value}""#,
                "sets a request header, replacing any earlier value of that name",
            ),
            Step::AddHeader => build_spec(
                r#"I add "{value}" to the "{name}" request header"#,
                "adds a value to a request header, keeping the earlier ones",
            ),
            Step::SetQuery => build_spec(
                r#"the query parameter "{name}" is "{value}""#,
                "sets a query parameter of the next request",
            ),
            Step::SetBody => build_spec(
                "the request body is:",
                "sets the raw body of the next request from the doc string",
            ),
            Step::EmptyBody => build_spec(
                "the request body is empty",
                "removes the body and the form of the next request",
            ),
            Step::SetForm => build_spec(
                "the request form parameters are:",
                "sets the form parameters of the next request from a name | value table",
            ),
            Step::Attach => build_spec(
                r#"I attach the file "{file}" to the request as "{name}""#,
                "attaches a file, path relative to this feature file, as a multipart/form-data part",
            ),
            Step::Hawk => build_spec(
                r#"I sign the next request with Hawk id "{id}" and key "{key:secret}""#,
                "signs the next request with these Hawk credentials, and every replay of it",
            ),
            Step::SrpVerifierWithSalt => srp_spec(
                r#"I generate an SRP verifier for "{username}" with password "{password:secret}" and salt "{salt}" as "{prefix}""#,
                "computes an SRP verifier from this salt into <prefix>_salt and <prefix>_verifier",
            ),
            Step::SrpVerifier => srp_spec(
                r#"I generate an SRP verifier for "{username}" with password "{password:secret}" as "{prefix}""#,
                "computes an SRP verifier from a fresh salt into <prefix>_salt and <prefix>_verifier",
            ),
            Step::SrpStartLogin => srp_spec(
                r#"I start an SRP login as "{prefix}""#,
                "starts an SRP login, storing the client values in <prefix>_a and <prefix>_A",
            ),
            Step::SrpCompleteLogin => {
                let mut spec = srp_spec(
                    r#"I complete SRP login "{prefix}" for "{username}" with password "{password:secret}" salt "{salt}" and "{server_public}""#,
                    "answers the server challenge into <prefix>_M1, <prefix>_M2 and <prefix>_sessionKey",
                );
                // It reads `<prefix>_a`, which the start wrote.
                spec["wants_vars"] = json!(true);
                spec
            }
        }
    }
}

/// The SRP steps write a family of variables named by their prefix.
fn srp_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "srp", "kind": "action",
        "description": description, "sets_vars": ["{prefix}_*"],
    })
}

fn send_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "action",
        "description": description, "writes": ["response"],
    })
}

fn assert_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "assertion",
        "description": description, "reads": ["response"], "sets_vars": [],
    })
}

fn build_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "action",
        "description": description, "writes": ["request"],
    })
}

pub fn list_json() -> String {
    Value::Array(ALL.iter().map(|step| step.spec()).collect()).to_string()
}

/// What a dispatch request holds of the step: its arguments, doc string and
/// table, and whether its evidence may be kept.
pub struct Call {
    args: Vec<String>,
    docstring: Option<String>,
    table: Option<Vec<Vec<String>>>,
    pub evidence: bool,
    /// 1-based within a polled assertion; 1 for a step that is not polled.
    attempt: u64,
    /// The directory of the file the step is written in.
    source_dir: String,
    /// The file's variables, for a step that asked for them.
    vars: Value,
}

impl Call {
    pub fn parse(request: &Value) -> Self {
        Self {
            args: request["args"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| a.as_str().unwrap_or_default().to_string())
                .collect(),
            docstring: request["docstring"].as_str().map(str::to_string),
            table: serde_json::from_value(request["table"].clone()).ok(),
            evidence: request["evidence"].as_bool().unwrap_or(false),
            attempt: request["attempt"].as_u64().unwrap_or(1),
            source_dir: request["source_dir"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            vars: request["vars"].clone(),
        }
    }

    fn arg(&self, index: usize) -> Result<&str, Reply> {
        self.need(index)
            .map_err(|_| Reply::failed(Class::Input, missing(index)))
    }

    fn need(&self, index: usize) -> Result<&str, AttemptError> {
        self.args
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| AttemptError::input(missing(index)))
    }
}

fn missing(index: usize) -> String {
    format!("the step is missing argument {}", index + 1)
}

/// Runs a step against the handle's state. Every failure is classed where it
/// is born.
pub fn run(step: Step, state: &mut State, call: &Call) -> Reply {
    match run_step(step, state, call) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn run_step(step: Step, state: &mut State, call: &Call) -> Result<Reply, Reply> {
    match step {
        Step::Request => Ok(request(state, call, "GET", call.arg(0)?)),
        Step::RequestUsing => Ok(request(state, call, call.arg(1)?, call.arg(0)?)),
        Step::SetHeader => Ok(header_step(state, call.arg(0)?, call.arg(1)?, true)),
        Step::AddHeader => Ok(header_step(state, call.arg(1)?, call.arg(0)?, false)),
        Step::SetQuery => {
            let (name, value) = (call.arg(0)?, call.arg(1)?);
            state.set_query(name, value);
            let mut reply = Reply::passed();
            reply
                .scan
                .push(json!({"kind": "query", "pairs": [[name, value]]}));
            Ok(reply)
        }
        Step::SetBody => set_body(state, call),
        Step::EmptyBody => {
            state.set_body(None);
            Ok(Reply::passed())
        }
        Step::SetForm => set_form(state, call),
        Step::Attach => {
            let file = upload::read(call.arg(0)?, call.arg(1)?, &call.source_dir)
                .map_err(|error| Reply::failed(Class::Input, error))?;
            state.attach(file);
            Ok(Reply::passed())
        }
        Step::Hawk => {
            state.sign_next(call.arg(0)?, call.arg(1)?);
            Ok(Reply::passed())
        }
        Step::SrpVerifier => Ok(login::generate_verifier(
            &*srp_params(state)?,
            call.arg(0)?,
            call.arg(1)?,
            None,
            call.arg(2)?,
        )),
        Step::SrpVerifierWithSalt => Ok(login::generate_verifier(
            &*srp_params(state)?,
            call.arg(0)?,
            call.arg(1)?,
            Some(call.arg(2)?),
            call.arg(3)?,
        )),
        Step::SrpStartLogin => Ok(login::start_login(&*srp_params(state)?, call.arg(0)?)),
        Step::SrpCompleteLogin => {
            let prefix = call.arg(0)?;
            Ok(login::complete_login(
                &*srp_params(state)?,
                &login::Completion {
                    prefix,
                    identity: call.arg(1)?,
                    password: call.arg(2)?,
                    salt: call.arg(3)?,
                    server_public: call.arg(4)?,
                    a: call.vars[format!("{prefix}_a")]["value"].as_str(),
                },
            ))
        }
        Step::ResponseCode
        | Step::ResponseBodyContainsJson
        | Step::ResponseBodyEqualsJson
        | Step::ResponseArrayLength
        | Step::ResponseHeader
        | Step::ResponseHeaderContains
        | Step::ResponseHeaderNotContains
        | Step::ResponseHeaderMatches
        | Step::ResponseHeaderNotMatches
        | Step::JsonNodeExists
        | Step::JsonNodeNotExists
        | Step::JsonNodeIsNull
        | Step::JsonNodeNotNull
        | Step::ResponseBodyNotContainsJson
        | Step::JsonNodeNotContainsSubstring
        | Step::ResponseBodyEmpty
        | Step::ResponseBodyContains
        | Step::ResponseBodyNotContains
        | Step::ResponseBodyMatches
        | Step::ResponseBodyNotMatches
        | Step::ResponseBodyHasElement
        | Step::ResponseBodyHasElementWithText
        | Step::ResponseBodyNotHasElement => Ok(response_assertion(step, state, call)),
        Step::ExtractJson => extract(state, call, read::extract_json, false),
        Step::ExtractJsonGlobal => extract(state, call, read::extract_json, true),
        Step::ExtractCookie => extract(state, call, read::extract_cookie, false),
        Step::ExtractCookieGlobal => extract(state, call, read::extract_cookie, true),
        Step::ExtractMarkup => extract(state, call, read::select, false),
        Step::PrintHeaders => print(state, |ex| Ok(read::print_headers(ex))),
        Step::PrintBody => print(state, read::print_body),
        Step::PrintBodyAs => {
            let selector = call.arg(0)?;
            print(state, |ex| read::print_body_as(ex, selector))
        }
    }
}

/// The selected `srp` instance's parameters.
fn srp_params(state: &State) -> Result<std::sync::Arc<crate::srp::SrpParams>, Reply> {
    state
        .srp()
        .map_err(|error| Reply::failed(Class::Input, error))
}

/// Before the first request: a literal attachment path must be a file.
/// Nothing else can be refused without a value.
pub fn validate(step: Step, request: &Value) -> Result<(), String> {
    if step != Step::Attach {
        return Ok(());
    }
    let file = request["args"][0].as_str().unwrap_or_default();
    upload::validate(file, request["source_dir"].as_str().unwrap_or_default())
}

/// The last response, for a step that only reads it. Never replayed.
fn last_response(state: &State) -> Result<&Exchange, Reply> {
    state
        .exchange
        .as_ref()
        .ok_or_else(|| Reply::failed(Class::Input, "no request has been sent yet"))
}

/// `extract "<arg 0>" … as "<name>"`: one value of the response into a variable.
fn extract(
    state: &State,
    call: &Call,
    read: fn(&Exchange, &str) -> Result<String, read::Failure>,
    global: bool,
) -> Result<Reply, Reply> {
    let ex = last_response(state)?;
    let (name, from) = (call.arg(1)?, call.arg(0)?);
    read(ex, from)
        .map(|value| read::stored(name, value, global))
        .map_err(|(class, message)| Reply::failed(class, message))
}

fn print(
    state: &State,
    item: impl FnOnce(&Exchange) -> Result<Value, read::Failure>,
) -> Result<Reply, Reply> {
    item(last_response(state)?)
        .map(read::printed)
        .map_err(|(class, message)| Reply::failed(class, message))
}

/// Attempt 1 checks the exchange the request left. From attempt 2 the request
/// is sent again first, to the API it went to, whatever instance is selected
/// now. The reply of a send that got no answer is what ends the attempt.
fn observe(state: &mut State, attempt: u64) -> Result<Reply, Reply> {
    if attempt <= 1 {
        return Ok(Reply::passed());
    }
    let recipe = state
        .recipe
        .clone()
        .ok_or_else(|| Reply::failed(Class::Input, "request has not been sent"))?;
    let api = state
        .api_named(&recipe.api)
        .map_err(|error| Reply::failed(Class::Input, error))?;
    state.exchange = None;
    let (sent, exchange) = send::send(&recipe.api, &recipe, &api, false);
    if exchange.is_some() {
        state.exchange = exchange;
        return Ok(sent);
    }
    Err(match sent.failure() {
        Some((Class::Transport, error)) => {
            let error = error.to_string();
            sent.into_not_yet(Class::Transport, error, &recipe.api)
        }
        _ => sent,
    })
}

fn response_assertion(step: Step, state: &mut State, call: &Call) -> Reply {
    let replayed = call.attempt > 1;
    let base = match observe(state, call.attempt) {
        Ok(base) => base,
        Err(reply) => return reply,
    };
    let Some(exchange) = state.exchange.as_ref() else {
        return Reply::failed(Class::Input, "no request has been sent yet");
    };
    let result = check(step, exchange, call);
    let mut reply = match result {
        Ok(()) => base,
        Err(AttemptError::NotYet(error)) => {
            base.into_not_yet(Class::Assertion, error, &exchange.api)
        }
        Err(AttemptError::Fatal(class, error)) => base.into_failure(class, error),
    };
    // A replay is a new exchange, shown with the reply it belongs to. The
    // first attempt's is shown by the request step, or by the failure context.
    if replayed && (call.evidence || reply.failure().is_some()) {
        reply.diagnostics.push(send::diagnostic(exchange));
    }
    reply
}

fn check(step: Step, ex: &Exchange, call: &Call) -> AttemptResult {
    let json = call.docstring.as_deref();
    match step {
        Step::ResponseCode => assert::response_code(ex, call.need(0)?),
        Step::ResponseBodyContainsJson => assert::body_contains_json(ex, json),
        Step::ResponseBodyEqualsJson => assert::body_equals_json(ex, json),
        Step::ResponseBodyNotContainsJson => assert::body_not_contains_json(ex, json),
        Step::ResponseArrayLength => assert::array_length(ex, call.need(0)?),
        Step::ResponseHeader => assert::response_header(ex, call.need(0)?, call.need(1)?),
        Step::ResponseHeaderContains => {
            assert::response_header_contains(ex, call.need(0)?, call.need(1)?, false)
        }
        Step::ResponseHeaderNotContains => {
            assert::response_header_contains(ex, call.need(0)?, call.need(1)?, true)
        }
        Step::ResponseHeaderMatches => {
            assert::response_header_matches(ex, call.need(0)?, call.need(1)?, false)
        }
        Step::ResponseHeaderNotMatches => {
            assert::response_header_matches(ex, call.need(0)?, call.need(1)?, true)
        }
        Step::JsonNodeExists => assert::json_node_exists(ex, call.need(0)?),
        Step::JsonNodeNotExists => assert::json_node_not_exists(ex, call.need(0)?),
        Step::JsonNodeIsNull => assert::json_node_is_null(ex, call.need(0)?),
        Step::JsonNodeNotNull => assert::json_node_not_null(ex, call.need(0)?),
        Step::JsonNodeNotContainsSubstring => {
            assert::json_node_not_contains(ex, call.need(0)?, call.need(1)?)
        }
        Step::ResponseBodyEmpty => assert::body_empty(ex),
        Step::ResponseBodyContains => assert::body_contains_text(ex, call.need(0)?, false),
        Step::ResponseBodyNotContains => assert::body_contains_text(ex, call.need(0)?, true),
        Step::ResponseBodyMatches => assert::body_matches(ex, call.need(0)?, false),
        Step::ResponseBodyNotMatches => assert::body_matches(ex, call.need(0)?, true),
        Step::ResponseBodyHasElement => assert::body_has_element(ex, call.need(0)?),
        Step::ResponseBodyHasElementWithText => {
            assert::body_has_element_with_text(ex, call.need(0)?, call.need(1)?)
        }
        Step::ResponseBodyNotHasElement => assert::body_not_has_element(ex, call.need(0)?),
        Step::Request
        | Step::RequestUsing
        | Step::SetHeader
        | Step::AddHeader
        | Step::SetQuery
        | Step::SetBody
        | Step::EmptyBody
        | Step::SetForm
        | Step::Attach
        | Step::Hawk
        | Step::SrpVerifier
        | Step::SrpVerifierWithSalt
        | Step::SrpStartLogin
        | Step::SrpCompleteLogin
        | Step::ExtractJson
        | Step::ExtractJsonGlobal
        | Step::ExtractCookie
        | Step::ExtractCookieGlobal
        | Step::ExtractMarkup
        | Step::PrintHeaders
        | Step::PrintBody
        | Step::PrintBodyAs => Err(AttemptError::input("the step is not an assertion")),
    }
}

/// A sensitive header's value is registered by the step that carries it, so a
/// scenario that fails before it sends does not print it.
fn header_step(state: &mut State, name: &str, value: &str, replace: bool) -> Reply {
    if replace {
        state.set_header(name, value);
    } else {
        state.add_header(name, value);
    }
    let mut reply = Reply::passed();
    if state.is_sensitive(name) {
        reply.secrets = header::secret_values(name, value);
    }
    reply
}

fn set_body(state: &mut State, call: &Call) -> Result<Reply, Reply> {
    let body = call.docstring.clone().ok_or_else(|| {
        Reply::failed(
            Class::Input,
            "step `the request body is:` requires a doc string",
        )
    })?;
    let mut reply = Reply::passed();
    reply.scan.push(json!({"kind": "json", "content": body}));
    state.set_body(Some(body));
    Ok(reply)
}

fn set_form(state: &mut State, call: &Call) -> Result<Reply, Reply> {
    let input = |message: &str| Reply::failed(Class::Input, message);
    let rows = call
        .table
        .as_ref()
        .ok_or_else(|| input("step `the request form parameters are:` requires a table"))?;
    let (head, rest) = rows.split_first().ok_or_else(|| input("table is empty"))?;
    if head.as_slice() != ["name", "value"] {
        return Err(input("table must have exactly two columns: name | value"));
    }
    let mut pairs = Vec::with_capacity(rest.len());
    for row in rest {
        let [name, value] = row.as_slice() else {
            return Err(input("table must have exactly two columns: name | value"));
        };
        pairs.push((name.clone(), value.clone()));
    }
    let mut reply = Reply::passed();
    reply.scan.push(json!({"kind": "form", "pairs": pairs}));
    state.set_form(pairs);
    Ok(reply)
}

fn request(state: &mut State, call: &Call, method: &str, path: &str) -> Reply {
    let (recipe, api) = match state.begin_send(method, path) {
        Ok(sent) => sent,
        Err(error) => return Reply::failed(Class::Input, error),
    };
    let (reply, exchange) = send::send(&recipe.api, &recipe, &api, call.evidence);
    if let Some(exchange) = exchange {
        state.recipe = Some(recipe);
        state.exchange = Some(exchange);
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_step_has_an_index_and_the_next_one_has_none() {
        let listed: Vec<Value> = serde_json::from_str(&list_json()).expect("JSON");
        assert_eq!(listed.len(), ALL.len());
        assert!(Step::from_index(u32::try_from(ALL.len()).expect("small")).is_none());
        for (index, spec) in listed.iter().enumerate() {
            let step = Step::from_index(u32::try_from(index).expect("small")).expect("step");
            assert_eq!(spec["expression"], step.spec()["expression"]);
        }
    }

    #[test]
    fn failed_send_leaves_no_stale_exchange() {
        let api =
            crate::config::Api::parse(&json!({"base_url": "http://127.0.0.1:1/"})).expect("valid");
        let mut state = State::new(std::collections::BTreeMap::from([(
            "main".to_string(),
            crate::state::Instance::Api(std::sync::Arc::new(api)),
        )]));
        state.begin("main").expect("ok");
        let (recipe, _) = state.begin_send("GET", "/p").expect("ok");
        state.recipe = Some(recipe);
        state.exchange = Some(crate::state::Exchange {
            method: "GET".into(),
            url: "http://earlier/".into(),
            request_headers: Vec::new(),
            request_body: None,
            request_form: crate::state::BodyForm::Raw,
            status: 200,
            response_headers: Vec::new(),
            response_body: String::new(),
            duration_us: 0,
            api: "main".into(),
            sensitive_headers: Vec::new(),
        });
        let call = Call::parse(&json!({"args": ["/p", "BAD METHOD"]}));
        let reply = run(Step::RequestUsing, &mut state, &call);
        assert!(reply.json().contains("\"fatal\""));
        assert!(state.recipe.is_none() && state.exchange.is_none());
    }

    fn state_with_a_request_to(base: &str) -> State {
        let api = crate::config::Api::parse(&json!({"base_url": base})).expect("valid");
        let mut state = State::new(std::collections::BTreeMap::from([(
            "main".to_string(),
            crate::state::Instance::Api(std::sync::Arc::new(api)),
        )]));
        state.begin("main").expect("ok");
        let (recipe, _) = state.begin_send("GET", "/p").expect("ok");
        state.recipe = Some(recipe);
        state
    }

    fn reply_of(step: Step, state: &mut State, request: &Value) -> Value {
        serde_json::from_str(&run(step, state, &Call::parse(request)).json()).expect("JSON")
    }

    #[test]
    fn a_replay_that_cannot_connect_is_not_yet_transport_and_keeps_the_recipe() {
        let mut state = state_with_a_request_to("http://127.0.0.1:1/");
        let v = reply_of(
            Step::ResponseCode,
            &mut state,
            &json!({"args": ["200"], "attempt": 2}),
        );
        assert_eq!(v["status"], "not_yet");
        assert_eq!(v["class"], "transport");
        assert_eq!(v["options_instance"], "main");
        assert!(state.exchange.is_none() && state.recipe.is_some());
    }

    #[test]
    fn nothing_sent_is_fatal_input_with_the_text_of_the_attempt() {
        let mut state = state_with_a_request_to("http://127.0.0.1:1/");
        state.recipe = None;
        for (attempt, text) in [
            (1, "no request has been sent yet"),
            (2, "request has not been sent"),
        ] {
            let v = reply_of(
                Step::ResponseCode,
                &mut state,
                &json!({"args": ["200"], "attempt": attempt}),
            );
            assert_eq!(
                (v["status"].as_str(), v["class"].as_str()),
                (Some("fatal"), Some("input"))
            );
            assert_eq!(v["error"], text);
        }
    }

    #[test]
    fn a_mismatch_may_change_and_one_that_cannot_is_fatal_assertion() {
        let mut state = state_with_a_request_to("http://127.0.0.1:1/");
        state.exchange = Some(crate::state::Exchange {
            method: "GET".into(),
            url: "http://earlier/".into(),
            request_headers: Vec::new(),
            request_body: None,
            request_form: crate::state::BodyForm::Raw,
            status: 404,
            response_headers: Vec::new(),
            response_body: r#"{"n": 1}"#.into(),
            duration_us: 0,
            api: "main".into(),
            sensitive_headers: Vec::new(),
        });
        let v = reply_of(Step::ResponseCode, &mut state, &json!({"args": ["200"]}));
        assert_eq!(
            (v["status"].as_str(), v["class"].as_str()),
            (Some("not_yet"), Some("assertion"))
        );
        assert_eq!(v["options_instance"], "main");
        assert!(v["diagnostics"].as_array().is_some_and(Vec::is_empty));
        let v = reply_of(
            Step::JsonNodeNotContainsSubstring,
            &mut state,
            &json!({"args": ["n", "1"]}),
        );
        assert_eq!(
            (v["status"].as_str(), v["class"].as_str()),
            (Some("fatal"), Some("assertion"))
        );
    }

    #[test]
    fn a_replay_that_cannot_connect_keeps_what_the_request_registered() {
        let mut state = state_with_a_request_to("http://127.0.0.1:1/");
        if let Some(recipe) = state.recipe.as_mut() {
            recipe
                .headers
                .push(("Authorization".into(), "Bearer abc1234".into()));
            recipe.body = Some(r#"{"token":"t"}"#.into());
        }
        let v = reply_of(
            Step::ResponseCode,
            &mut state,
            &json!({"args": ["200"], "attempt": 2}),
        );
        assert!(v["secrets"].to_string().contains("abc1234"), "{v}");
        assert!(v["scan"].to_string().contains(r#"\"token\""#), "{v}");
    }

    #[test]
    fn a_step_that_sends_writes_the_response_and_one_that_builds_writes_the_request() {
        let listed: Vec<Value> = serde_json::from_str(&list_json()).expect("JSON");
        for spec in &listed {
            let text = spec["expression"].as_str().expect("expression");
            let writes = &spec["writes"];
            if text.starts_with("I request") {
                assert_eq!(writes, &json!(["response"]), "{text}");
            } else if spec["reads"] == json!(["response"]) {
                // An extract or a print reads the response and builds nothing.
                assert!(writes.is_null(), "{text}");
            } else if spec["group"] == "srp" {
                assert!(writes.is_null(), "{text}");
                assert_eq!(spec["sets_vars"], json!(["{prefix}_*"]), "{text}");
            } else if spec["kind"] == "action" {
                assert_eq!(writes, &json!(["request"]), "{text}");
            }
        }
    }
}
