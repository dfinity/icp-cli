use predicates::{
    prelude::*,
    reflection,
    reflection::{Child, Parameter, PredicateReflection},
};
use serde_json::Value;
use std::fmt;

type JsonFieldValueCheck<'a> = Box<dyn Fn(&Value) -> Result<(), String> + 'a>;

/// The actual predicate object that implements `Predicate<str>`
pub struct JsonFieldPredicate<'a> {
    pub(crate) field: &'a str,
    pub(crate) description: String,
    pub(crate) check: JsonFieldValueCheck<'a>,
}

impl<'a> Predicate<str> for JsonFieldPredicate<'a> {
    fn eval(&self, actual: &str) -> bool {
        match serde_json::from_str::<Value>(actual) {
            Ok(json) => match json.get(self.field) {
                Some(val) => (self.check)(val).is_ok(),
                None => false,
            },
            Err(_) => false,
        }
    }

    fn find_case<'a_, 'b>(
        &'a_ self,
        _expected: bool,
        actual: &'b str,
    ) -> Option<reflection::Case<'a>> {
        match serde_json::from_str::<Value>(actual) {
            Err(e) => leaked_diag(format!("Could not parse JSON: {}", e)),
            Ok(json) => match json.get(self.field) {
                None => leaked_diag(format!("Missing field '{}'", self.field)),
                Some(val) => match (self.check)(val) {
                    Ok(_) => None, // pass
                    Err(failure_msg) => leaked_diag(failure_msg),
                },
            },
        }
    }
}

impl PredicateReflection for JsonFieldPredicate<'_> {
    fn parameters<'b>(&'b self) -> Box<dyn Iterator<Item = Parameter<'b>> + 'b> {
        Box::new(vec![].into_iter())
    }

    fn children<'b>(&'b self) -> Box<dyn Iterator<Item = Child<'b>> + 'b> {
        Box::new(vec![].into_iter())
    }
}

impl fmt::Display for JsonFieldPredicate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field '{}' {}", self.field, self.description)
    }
}

struct JsonFieldDiagnostic {
    msg: String,
}

impl fmt::Display for JsonFieldDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl PredicateReflection for JsonFieldDiagnostic {
    fn parameters<'a>(&'a self) -> Box<dyn Iterator<Item = Parameter<'a>> + 'a> {
        Box::new(std::iter::empty())
    }

    fn children<'a>(&'a self) -> Box<dyn Iterator<Item = Child<'a>> + 'a> {
        Box::new(std::iter::empty())
    }
}

fn leaked_diag(msg: String) -> Option<reflection::Case<'static>> {
    Some(reflection::Case::new(
        Some(Box::leak(Box::new(JsonFieldDiagnostic { msg }))),
        false,
    ))
}
