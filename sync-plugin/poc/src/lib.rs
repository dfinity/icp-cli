wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../crates/icp-sync-plugin/sync-plugin.wit",
});

use std::fs;
use std::path::Path;

use candid::Encode;

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        println!(
            "sync plugin: starting for canister {} (environment: {})",
            input.canister_id, input.environment
        );

        // 1. Upload the config value — the first file the manifest declared.
        if let Some(config) = input.files.first() {
            let arg = Encode!(&config.content.trim())
                .map_err(|e| format!("encode set_config arg: {e}"))?;
            canister_call(&CanisterCallRequest {
                method: "set_config".to_string(),
                arg,
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            println!("set_config from {}: ok", config.name);
        }

        // 2. Register every file found by traversing the preopened dirs.
        let mut registered = 0u32;
        for dir in &input.dirs {
            registered += register_dir(Path::new(dir))?;
        }

        Ok(Some(format!(
            "registered {} item(s) in canister {} (environment: {})",
            registered, input.canister_id, input.environment
        )))
    }
}

fn register_dir(dir: &Path) -> Result<u32, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    let mut count = 0u32;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file_type {}: {e}", path.display()))?;
        if file_type.is_dir() {
            count += register_dir(&path)?;
        } else if file_type.is_file() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("read_to_string {}: {e}", path.display()))?;
            let path_str = path.to_string_lossy().into_owned();
            let content_trimmed = content.trim();
            let arg = Encode!(&path_str, &content_trimmed)
                .map_err(|e| format!("encode register arg: {e}"))?;
            canister_call(&CanisterCallRequest {
                method: "register".to_string(),
                arg,
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            println!("{path_str}: ok");
            count += 1;
        }
    }
    Ok(count)
}

export!(Plugin);
