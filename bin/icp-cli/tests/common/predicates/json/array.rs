use crate::common::predicates::json::predicate::JsonFieldPredicate;
use serde_json::Value;

/// Creates a builder for a predicate that checks whether a specific array field
/// in a JSON object satisfies a condition.
///
/// Use methods on the returned `JsonArrayField` to specify the condition.
///
/// Example, if output is something like { "foo": [1, 2, 3] }:
///     assert_cmd::Command::new("mycmd")
///        .assert()
///        .success()
///        .stdout(json_field("foo").array().equals(vec![1.into(), 2.into(), 3.into()]));
pub struct JsonArrayField<'a> {
    pub(crate) field_name: &'a str,
}

impl<'a> JsonArrayField<'a> {
    /// Creates a predicate that passes if the specified JSON field is an array
    /// and equals the given value.
    ///
    /// Fails if the field is missing, not an array, or not equal to `expected`.
    pub fn equals(self, expected: Vec<Value>) -> JsonFieldPredicate<'a> {
        JsonFieldPredicate {
            field: self.field_name,
            description: format!("equals array {}", compact_json_array(&expected)),
            check: Box::new(move |v| match v {
                Value::Array(actual) if *actual == expected => Ok(()),
                Value::Array(actual) => Err(format!(
                    concat!(
                        "equality check on JSON array field '{}'.\n",
                        "  Expected: {}\n",
                        "  Actual:   {}"
                    ),
                    &self.field_name,
                    compact_json_array(&expected),
                    compact_json_array(actual),
                )),
                _ => Err(format!("JSON field '{}' was not an array", self.field_name)),
            }),
        }
    }
}

fn compact_json_array(vals: &[Value]) -> String {
    let elements: Vec<String> = vals
        .iter()
        .map(|v| {
            if let Value::Number(n) = v {
                n.to_string()
            } else {
                v.to_string()
            }
        })
        .collect();
    format!("[{}]", elements.join(", "))
}
