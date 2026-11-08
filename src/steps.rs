//! The steps this build declares. The index in `LIST` is the step's identity
//! at dispatch, so the array and `Step::from_index` live together.

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Request,
    ResponseCode,
}

const ALL: [Step; 2] = [Step::Request, Step::ResponseCode];

impl Step {
    pub fn from_index(index: u32) -> Option<Self> {
        ALL.get(usize::try_from(index).ok()?).copied()
    }

    fn spec(self) -> Value {
        match self {
            Step::Request => json!({
                "expression": r#"I request "{path}""#,
                "group": "api", "kind": "action",
                "description": "sends a GET request to the path, resolved against the API's base URL",
                "writes": ["response"],
            }),
            Step::ResponseCode => json!({
                "expression": "the response code is {code:uint}",
                "group": "api", "kind": "assertion",
                "description": "asserts the status code of the last response",
                "reads": ["response"],
                "sets_vars": [],
            }),
        }
    }
}

pub fn list_json() -> String {
    Value::Array(ALL.iter().map(|step| step.spec()).collect()).to_string()
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
}
