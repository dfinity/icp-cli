use icp::manifest::project::ProjectInner;
use schemars::schema_for;

const ID: &str = "https://dfinity.org/schemas/icp-yaml/v1.0.0";
const TITLE: &str = "ICP Project Configuration";
const DESCRIPTION: &str = "Schema for icp.yaml project configuration files used by the ICP CLI";

/// Generate JSON Schema for icp.yaml configuration files
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate schema for the main ProjectManifest type
    let schema = schema_for!(ProjectInner);

    // Add metadata to the schema
    let mut schema_json = serde_json::to_value(schema)?;

    // Add custom schema metadata
    if let Some(schema_obj) = schema_json.as_object_mut() {
        schema_obj.insert(
            "$id".to_string(),                         // k
            serde_json::Value::String(ID.to_string()), // v
        );

        schema_obj.insert(
            "title".to_string(),
            serde_json::Value::String(TITLE.to_string()),
        );

        schema_obj.insert(
            "description".to_string(),
            serde_json::Value::String(DESCRIPTION.to_string()),
        );
    }

    // Pretty print the JSON schema to stdout
    println!("{}", serde_json::to_string_pretty(&schema_json)?);

    Ok(())
}
