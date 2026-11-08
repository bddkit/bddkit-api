//! What one handle holds for a feature file: which instance of its group is
//! current, the request being built, and the last request sent with what came
//! back. One handle serves every instance of its group (`handle_scope:
//! group`), so the last response survives `I use "<name>" api`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::config::Api;
use crate::srp::SrpParams;
use crate::upload::FilePart;

/// Name and value pairs, in the order they are sent.
pub type Pairs = Vec<(String, String)>;

/// Hawk credentials for the request they are armed for.
#[derive(Clone)]
pub struct Credentials {
    pub id: String,
    pub key: String,
}

/// The request the next send is made of.
#[derive(Default)]
pub struct Pending {
    pub headers: Pairs,
    pub query: Pairs,
    pub body: Option<String>,
    pub form: Option<Pairs>,
    pub files: Vec<FilePart>,
    /// Signs the next distinct send only; a replay keeps them in its recipe.
    pub signer: Option<Credentials>,
}

/// A request as sent: the resource it went to, by name, and everything it was
/// made of, so it can be sent again.
#[derive(Clone)]
pub struct Recipe {
    pub api: String,
    pub method: String,
    pub path: String,
    pub query: Pairs,
    pub headers: Pairs,
    pub body: Option<String>,
    pub form: Option<Pairs>,
    pub files: Vec<FilePart>,
    pub signer: Option<Credentials>,
}

/// How a request body is laid out, which tells the host where its sensitive
/// fields stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyForm {
    Raw,
    Form,
    Multipart,
}

impl BodyForm {
    pub fn as_str(self) -> &'static str {
        match self {
            BodyForm::Raw => "raw",
            BodyForm::Form => "form",
            BodyForm::Multipart => "multipart",
        }
    }
}

/// The last request and response, as the host's `http` diagnostic holds them.
#[derive(Clone)]
pub struct Exchange {
    pub method: String,
    pub url: String,
    pub request_headers: Pairs,
    pub request_body: Option<String>,
    pub request_form: BodyForm,
    pub status: u16,
    pub response_headers: Pairs,
    pub response_body: String,
    pub duration_us: u64,
    pub api: String,
    pub sensitive_headers: Vec<String>,
}

/// One declared instance, checked: an `api` or an `srp` of the handle's group.
pub enum Instance {
    Api(Arc<Api>),
    Srp(Arc<SrpParams>),
}

impl Instance {
    fn api(&self) -> Option<&Arc<Api>> {
        match self {
            Instance::Api(api) => Some(api),
            Instance::Srp(_) => None,
        }
    }
}

pub struct State {
    apis: BTreeMap<String, Instance>,
    current: Option<String>,
    pub pending: Pending,
    pub recipe: Option<Recipe>,
    pub exchange: Option<Exchange>,
}

impl State {
    pub fn new(apis: BTreeMap<String, Instance>) -> Self {
        Self {
            apis,
            current: None,
            pending: Pending::default(),
            recipe: None,
            exchange: None,
        }
    }

    /// The instance a dispatch names. The first dispatch after the handle is
    /// built or reset sets the starting point, because the host only records a
    /// switch made before the handle existed; later ones find it set already
    /// and change nothing, since the host tells the plugin of every switch.
    pub fn begin(&mut self, instance: &str) -> Result<(), String> {
        if self.current.is_none() {
            self.switch_to(instance)?;
        }
        Ok(())
    }

    /// `I use "<name>" api`: the new instance's `default_headers` replace the
    /// pending ones. Everything else the next request holds is cleared, and
    /// the last request and exchange are kept.
    pub fn switch_to(&mut self, instance: &str) -> Result<(), String> {
        let api = self
            .apis
            .get(instance)
            .ok_or_else(|| format!("no instance \"{instance}\" is declared"))?;
        self.pending = Pending {
            headers: api
                .api()
                .iter()
                .flat_map(|api| api.default_headers.clone())
                .collect(),
            ..Pending::default()
        };
        self.current = Some(instance.to_string());
        Ok(())
    }

    /// The scenario boundary: the next dispatch starts again from its default.
    pub fn reset(&mut self) {
        self.current = None;
        self.pending = Pending::default();
        self.recipe = None;
        self.exchange = None;
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// The selected instance's name and checked config.
    pub fn selected(&self) -> Result<(&str, &Arc<Api>), String> {
        let name = self.current().ok_or("no API instance is selected")?;
        let api = self
            .apis
            .get(name)
            .and_then(Instance::api)
            .ok_or_else(|| format!("\"{name}\" is not an API instance"))?;
        Ok((name, api))
    }

    /// The selected `srp` instance's parameters.
    pub fn srp(&self) -> Result<Arc<SrpParams>, String> {
        let name = self.current().ok_or("no SRP instance is selected")?;
        match self.apis.get(name) {
            Some(Instance::Srp(params)) => Ok(Arc::clone(params)),
            _ => Err(format!("\"{name}\" is not an SRP instance")),
        }
    }

    /// An instance's checked config by name, selected or not: a replay goes to
    /// the API its request went to.
    pub fn api_named(&self, name: &str) -> Result<Arc<Api>, String> {
        self.apis
            .get(name)
            .and_then(Instance::api)
            .map(Arc::clone)
            .ok_or_else(|| format!("\"{name}\" is not an API instance"))
    }

    /// What a failing step adds to its dump: the last exchange, once there is
    /// one. A handle without an API instance (`srp`) answers nothing, so its
    /// steps never show an exchange a second time.
    pub fn failure_context(&self) -> Vec<serde_json::Value> {
        if !self.apis.values().any(|instance| instance.api().is_some()) {
            return Vec::new();
        }
        self.exchange.iter().map(crate::send::diagnostic).collect()
    }

    /// Whether the selected instance masks this header's value.
    pub fn is_sensitive(&self, header: &str) -> bool {
        self.selected().is_ok_and(|(_, api)| {
            api.sensitive_headers
                .iter()
                .any(|masked| masked.eq_ignore_ascii_case(header))
        })
    }

    /// Replace: removes every value with this name, then sets one.
    pub fn set_header(&mut self, name: &str, value: &str) {
        self.pending
            .headers
            .retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        self.add_header(name, value);
    }

    pub fn add_header(&mut self, name: &str, value: &str) {
        self.pending
            .headers
            .push((name.to_string(), value.to_string()));
    }

    pub fn set_query(&mut self, name: &str, value: &str) {
        self.pending.query.retain(|(k, _)| k != name);
        self.pending
            .query
            .push((name.to_string(), value.to_string()));
    }

    pub fn set_body(&mut self, body: Option<String>) {
        self.pending.body = body;
        self.pending.form = None;
        self.pending.files.clear();
    }

    /// Any attachment turns the request into `multipart/form-data`, with the
    /// form parameters as its text parts.
    pub fn attach(&mut self, file: FilePart) {
        self.pending.body = None;
        self.pending.files.push(file);
    }

    /// Armed for the next send only.
    pub fn sign_next(&mut self, id: &str, key: &str) {
        self.pending.signer = Some(Credentials {
            id: id.to_string(),
            key: key.to_string(),
        });
    }

    pub fn set_form(&mut self, pairs: Pairs) {
        self.pending.form = Some(pairs);
        self.pending.body = None;
    }

    /// A send forgets the last request and its exchange before it builds
    /// anything, so a send that fails for any reason leaves nothing earlier
    /// to dump. Returns what is to be sent and the resource it goes to.
    pub fn begin_send(&mut self, method: &str, path: &str) -> Result<(Recipe, Arc<Api>), String> {
        self.recipe = None;
        self.exchange = None;
        let (name, api) = self.selected()?;
        let (name, api) = (name.to_string(), Arc::clone(api));
        let recipe = Recipe {
            api: name,
            method: method.to_string(),
            path: path.to_string(),
            query: self.pending.query.clone(),
            headers: self.pending.headers.clone(),
            body: self.pending.body.clone(),
            form: self.pending.form.clone(),
            files: self.pending.files.clone(),
            signer: self.pending.signer.take(),
        };
        Ok((recipe, api))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api(header: (&str, &str)) -> Instance {
        Instance::Api(Arc::new(
            Api::parse(&json!({"base_url": "http://x/", "default_headers": {header.0: header.1}}))
                .expect("valid"),
        ))
    }

    fn srp_instance() -> Instance {
        Instance::Srp(Arc::new(
            SrpParams::from_config(&json!({"variant": "hex-string"})).expect("valid"),
        ))
    }

    fn state() -> State {
        State::new(BTreeMap::from([
            ("main".to_string(), api(("x-a", "1"))),
            ("other".to_string(), api(("x-b", "2"))),
        ]))
    }

    fn an_exchange() -> Exchange {
        Exchange {
            method: "GET".into(),
            url: "http://x/".into(),
            request_headers: Vec::new(),
            request_body: None,
            request_form: BodyForm::Raw,
            status: 200,
            response_headers: Vec::new(),
            response_body: String::new(),
            duration_us: 0,
            api: "main".into(),
            sensitive_headers: Vec::new(),
        }
    }

    #[test]
    fn the_failure_context_is_the_last_exchange_of_an_api_handle_and_nothing_else() {
        let mut s = state();
        assert!(s.failure_context().is_empty(), "nothing sent yet");
        s.exchange = Some(an_exchange());
        let context = s.failure_context();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0]["kind"], "http");
        assert_eq!(context[0]["resource"], "main");
        let mut srp = State::new(BTreeMap::from([("main".to_string(), srp_instance())]));
        srp.exchange = Some(an_exchange());
        assert!(srp.failure_context().is_empty(), "an srp handle shows none");
    }

    #[test]
    fn new_state_seeds_headers_from_the_resource_defaults() {
        let mut s = state();
        s.begin("main").expect("ok");
        assert_eq!(s.current(), Some("main"));
        assert_eq!(s.pending.headers, [("x-a".to_string(), "1".to_string())]);
    }

    #[test]
    fn use_api_replaces_headers_with_the_new_resource_defaults() {
        let mut s = state();
        s.begin("main").expect("ok");
        s.switch_to("other").expect("ok");
        s.begin("other").expect("ok");
        assert_eq!(s.current(), Some("other"));
        assert_eq!(s.pending.headers, [("x-b".to_string(), "2".to_string())]);
    }

    #[test]
    fn reset_returns_to_the_default_resource() {
        let mut s = state();
        s.switch_to("other").expect("ok");
        s.set_query("a", "1");
        s.begin_send("GET", "/").expect("ok");
        s.reset();
        assert_eq!(s.current(), None);
        assert!(s.pending.query.is_empty() && s.recipe.is_none());
        s.begin("main").expect("ok");
        assert_eq!(s.current(), Some("main"));
    }

    #[test]
    fn use_api_with_an_undeclared_name_is_an_error() {
        let mut s = state();
        s.begin("main").expect("ok");
        assert!(s.switch_to("nope").is_err());
        assert_eq!(s.current(), Some("main"));
    }

    #[test]
    fn use_api_clears_the_pending_body_and_query_and_keeps_the_last_request() {
        let mut s = state();
        s.begin("main").expect("ok");
        s.set_query("a", "1");
        s.set_body(Some("{}".into()));
        let (recipe, _) = s.begin_send("GET", "/p").expect("ok");
        s.recipe = Some(recipe);
        s.switch_to("other").expect("ok");
        assert!(s.pending.query.is_empty() && s.pending.body.is_none());
        assert_eq!(s.recipe.as_ref().map(|r| r.api.as_str()), Some("main"));
    }

    #[test]
    fn a_send_forgets_the_last_request_before_it_builds_anything() {
        let mut s = state();
        s.begin("main").expect("ok");
        let (recipe, _) = s.begin_send("GET", "/p").expect("ok");
        s.recipe = Some(recipe);
        s.begin_send("POST", "/q").expect("ok");
        assert!(s.recipe.is_none() && s.exchange.is_none());
    }

    #[test]
    fn set_header_replaces_existing_case_insensitively_and_add_header_appends() {
        let mut s = state();
        s.begin("main").expect("ok");
        s.set_header("X-A", "9");
        s.add_header("x-a", "10");
        assert_eq!(
            s.pending.headers,
            [
                ("X-A".to_string(), "9".to_string()),
                ("x-a".to_string(), "10".to_string())
            ]
        );
    }

    #[test]
    fn body_and_form_are_mutually_exclusive() {
        let mut s = state();
        s.set_body(Some("{}".into()));
        s.set_form(vec![("a".into(), "1".into())]);
        assert!(s.pending.body.is_none() && s.pending.form.is_some());
        s.set_body(Some("{}".into()));
        assert!(s.pending.form.is_none());
    }
}
