use icp::manifest::{CanisterManifest, EnvironmentManifest, NetworkManifest, ProjectManifest};

macro_rules! generate_schemas {
    ($base:expr, $($t:ty => $filename:expr),+ $(,)?) => {{
        let base : icp::prelude::PathBuf = $base.into();

        $(
            {
                let schema = schemars::schema_for!($t);
                let mut schema_json = serde_json::to_value(&schema).unwrap();

                if let Some(obj) = schema_json.as_object_mut() {
                    obj.insert("$id".to_string(), serde_json::Value::String(stringify!($t).to_string()));
                    obj.insert("title".to_string(), serde_json::Value::String(stringify!($t).to_string()));
                    obj.insert(
                        "description".to_string(),
                        serde_json::Value::String(format!("Schema for {}", stringify!($t))),
                    );
                }

                // Build the full path: base + filename
                let path = base.join($filename);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).expect("failed to create parent directories");
                }

                let pretty = serde_json::to_string_pretty(&schema_json).unwrap();
                std::fs::write(&path, pretty).expect("failed to write schema file");

                println!("✅ Wrote {} to {}", stringify!($t), path.to_string());
            }
        )+
    }};
}

fn main() {
    // Take the base directory as the first command-line argument
    let base = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: cargo run -- <base-path>");
        std::process::exit(1);
    });

    generate_schemas!(
        &base,
        ProjectManifest => "icp-yaml-schema.json",
        CanisterManifest => "canister-yaml-schema.json",
        NetworkManifest => "network-yaml-schema.json",
        EnvironmentManifest => "environment-yaml-schema.json",
    );
}
