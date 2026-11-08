//! The steps this build declares and what the ones that build and send a
//! request do. The index in `ALL` is the step's identity at dispatch, so the
//! array and `Step::from_index` live together.

use serde_json::{Value, json};

use crate::header;
use crate::reply::{Class, Reply};
use crate::send;
use crate::state::State;

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
}

const ALL: [Step; 11] = [
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
];

impl Step {
    pub fn from_index(index: u32) -> Option<Self> {
        ALL.get(usize::try_from(index).ok()?).copied()
    }

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
            Step::ResponseCode => json!({
                "expression": "the response code is {code:uint}",
                "group": "api", "kind": "assertion",
                "description": "asserts the status code of the last response",
                "reads": ["response"],
                "sets_vars": [],
            }),
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
                "attaches a file to the next request as a multipart part",
            ),
            Step::Hawk => build_spec(
                r#"I sign the next request with Hawk id "{id}" and key "{key:secret}""#,
                "signs the next request with Hawk",
            ),
        }
    }
}

fn send_spec(expression: &str, description: &str) -> Value {
    json!({
        "expression": expression, "group": "api", "kind": "action",
        "description": description, "writes": ["response"],
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
        }
    }

    fn arg(&self, index: usize) -> Result<&str, Reply> {
        self.args.get(index).map(String::as_str).ok_or_else(|| {
            Reply::failed(
                Class::Input,
                format!("the step is missing argument {}", index + 1),
            )
        })
    }
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
        Step::ResponseCode | Step::Attach | Step::Hawk => Err(Reply::failed(
            Class::Input,
            "this step is declared but not implemented in this build yet",
        )),
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
            Some(std::sync::Arc::new(api)),
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

    #[test]
    fn a_step_that_sends_writes_the_response_and_one_that_builds_writes_the_request() {
        let listed: Vec<Value> = serde_json::from_str(&list_json()).expect("JSON");
        for spec in &listed {
            let text = spec["expression"].as_str().expect("expression");
            let writes = &spec["writes"];
            if text.starts_with("I request") {
                assert_eq!(writes, &json!(["response"]), "{text}");
            } else if spec["kind"] == "action" {
                assert_eq!(writes, &json!(["request"]), "{text}");
            }
        }
    }
}
