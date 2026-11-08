//! What one handle holds for a feature file: which instance of its group is
//! current, and the headers the next request starts with. One handle serves
//! every instance of its group (`handle_scope: group`), so the last response
//! survives `I use "<name>" api`.

use std::collections::BTreeMap;

/// Header name and value, in the order they are sent.
pub type Headers = Vec<(String, String)>;

pub struct State {
    /// Each instance's `default_headers`.
    defaults: BTreeMap<String, Headers>,
    current: Option<String>,
    pending_headers: Headers,
}

impl State {
    pub fn new(defaults: BTreeMap<String, Headers>) -> Self {
        Self {
            defaults,
            current: None,
            pending_headers: Vec::new(),
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
    /// pending ones. Everything else the next request holds is cleared, and the
    /// last exchange is kept.
    pub fn switch_to(&mut self, instance: &str) -> Result<(), String> {
        let headers = self
            .defaults
            .get(instance)
            .ok_or_else(|| format!("no instance \"{instance}\" is declared"))?
            .clone();
        self.current = Some(instance.to_string());
        self.pending_headers = headers;
        Ok(())
    }

    /// The scenario boundary: the next dispatch starts again from its default.
    pub fn reset(&mut self) {
        self.current = None;
        self.pending_headers.clear();
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn pending_headers(&self) -> &[(String, String)] {
        &self.pending_headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(BTreeMap::from([
            (
                "main".to_string(),
                vec![("x-a".to_string(), "1".to_string())],
            ),
            (
                "other".to_string(),
                vec![("x-b".to_string(), "2".to_string())],
            ),
        ]))
    }

    #[test]
    fn the_first_dispatch_sets_the_starting_point_and_its_headers() {
        let mut s = state();
        s.begin("main").expect("ok");
        assert_eq!(s.current(), Some("main"));
        assert_eq!(s.pending_headers(), [("x-a".to_string(), "1".to_string())]);
    }

    #[test]
    fn a_later_dispatch_does_not_undo_a_switch() {
        let mut s = state();
        s.begin("main").expect("ok");
        s.switch_to("other").expect("ok");
        s.begin("other").expect("ok");
        assert_eq!(s.current(), Some("other"));
        assert_eq!(s.pending_headers(), [("x-b".to_string(), "2".to_string())]);
    }

    #[test]
    fn a_reset_forgets_the_switch() {
        let mut s = state();
        s.switch_to("other").expect("ok");
        s.reset();
        assert_eq!(s.current(), None);
        s.begin("main").expect("ok");
        assert_eq!(s.current(), Some("main"));
    }

    #[test]
    fn an_undeclared_instance_is_refused_and_changes_nothing() {
        let mut s = state();
        s.begin("main").expect("ok");
        assert!(s.switch_to("nope").is_err());
        assert_eq!(s.current(), Some("main"));
    }
}
