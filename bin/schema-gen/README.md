# Schema Generation Tool

This tool automatically generates JSON Schema definitions for the `icp.yaml` configuration format used by the ICP CLI.

## Overview

The schema is generated directly from the Rust type definitions in the codebase using the [`schemars`](https://docs.rs/schemars/) crate. This approach has several benefits:

## Usage

### Generate Schema

```bash
# From the workspace root
cargo run --bin schema-gen

# Or use the convenience script
./scripts/generate-schema.sh
```

This creates `icp-yaml-schema.json` in the workspace root.

### Adding New Configuration Types

When adding new configuration types:

1. Add the new Rust struct/enum with appropriate derives:
   ```rust
   #[derive(Clone, Debug, Deserialize, JsonSchema)]
   pub struct MyNewType {
       // fields...
   }
   ```

2. If the type contains fields that don't implement `JsonSchema`, use the `#[schemars(with = "...")]` attribute:
   ```rust
   #[derive(Clone, Debug, Deserialize, JsonSchema)]  
   pub struct MyType {
       #[schemars(with = "String")]
       pub path: Utf8PathBuf,
   }
   ```

3. Regenerate the schema:
   ```bash
   ./scripts/generate-schema.sh
   ```

## Dependencies

The schema generator depends on:

- `schemars` - JSON Schema generation from Rust types
- `serde_json` - JSON serialization for the output
- All ICP library crates that define configuration types

## Schema Customization

### Field-level Customization

Use `schemars` attributes to customize individual fields:

```rust
#[derive(JsonSchema)]
pub struct Example {
    #[schemars(description = "Custom description")]
    pub field1: String,
    
    #[schemars(with = "String")]  
    pub field2: Utf8PathBuf,
    
    #[schemars(skip)]
    pub field3: InternalType,
}
```

### Type-level Customization

Customize the entire type schema:

```rust
#[derive(JsonSchema)]
#[schemars(title = "Custom Title", description = "Custom description")]
pub struct Example {
    // fields...
}
```

### Advanced Customization

For more complex customization, implement `JsonSchema` manually:

```rust
use schemars::{JsonSchema, gen::SchemaGenerator, schema::Schema};

impl JsonSchema for MyCustomType {
    fn schema_name() -> String {
        "MyCustomType".to_owned()
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        // Custom schema generation logic
    }
}
```

## Output Format

The generated schema follows JSON Schema Draft 7 and includes:

- Complete type definitions for all configuration structures
- Field descriptions from Rust documentation comments
- Validation constraints from serde attributes
- Custom metadata (title, description, schema ID)

The schema can be used with any JSON Schema validator or IDE that supports JSON Schema validation for YAML files.
