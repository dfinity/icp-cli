use crate::common::predicates::json::predicate::JsonFieldPredicate;

/// Creates a builder for a predicate that checks whether a specific field
/// in a JSON object satisfies a condition.
///
/// Use methods on the returned `JsonStringField` to specify the condition.
///
/// Example, if output is something like { "status": "healthy" }:
///     assert_cmd::Command::new("mycmd")
//         .assert()
//         .success()
//         .stdout(json_field("status").string().equals("healthy"));
pub struct JsonStringField<'a> {
    pub(crate) field_name: &'a str,
}

impl<'a> JsonStringField<'a> {
    /// Creates a predicate that passes if the specified JSON field is a string
    /// and equals the given value.
    ///
    /// Fails if the field is missing, not a string, or not equal to `expected`.
    pub fn equals(self, expected: &'a str) -> JsonFieldPredicate<'a> {
        let expected = expected.to_string();
        JsonFieldPredicate {
            field: self.field_name,
            description: format!("equals string '{}'", expected),
            check: Box::new(move |v| match v.as_str() {
                Some(s) if s == expected => Ok(()),
                Some(actual) => Err(format!(
                    concat!(
                        "equality check on JSON string field '{}'.\n",
                        "  Expected: \"{}\"\n",
                        "  Actual:   \"{}\""
                    ),
                    &self.field_name, expected, actual
                )),
                None => Err(format!("JSON field '{}' was not a string", self.field_name)),
            }),
        }
    }
}
