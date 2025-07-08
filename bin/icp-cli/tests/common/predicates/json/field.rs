use crate::common::predicates::json::array::JsonArrayField;
use crate::common::predicates::json::string::JsonStringField;

/// Used to create a builder for a predicate that checks whether a specific field
/// in a JSON object satisfies a condition.
///
/// Example, if output is something like { "status": "healthy", "foo": [1, 2, 3] }:
///     assert_cmd::Command::new("mycmd")
///        .assert()
///        .success()
///        .stdout(json_field("status").string().equals("healthy"))
///        .stdout(json_field("foo").array().equals(vec![1.into(), 2.into(), 3.into()]));
pub fn json_field(field_name: &str) -> JsonField<'_> {
    JsonField { field_name }
}

pub struct JsonField<'a> {
    field_name: &'a str,
}

impl<'a> JsonField<'a> {
    pub fn string(self) -> JsonStringField<'a> {
        JsonStringField {
            field_name: self.field_name,
        }
    }

    pub fn array(self) -> JsonArrayField<'a> {
        JsonArrayField {
            field_name: self.field_name,
        }
    }
}
