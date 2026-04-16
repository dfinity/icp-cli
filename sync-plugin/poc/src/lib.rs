wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

use std::fs;
use std::path::Path;

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        log(&format!(
            "sync plugin: starting for canister {} (environment: {})",
            input.canister_id, input.environment
        ));

        // 1. Upload the config value — the first file the manifest declared.
        if let Some(config) = input.files.first() {
            canister_call(&CanisterCallRequest {
                method: "set_config".to_string(),
                arg: format!("(\"{}\")", escape_candid_text(config.content.trim())),
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            log(&format!("set_config from {}: ok", config.name));
        }

        // 2. Register every file found by traversing the preopened dirs.
        let mut registered = 0u32;
        for dir in &input.dirs {
            registered += register_dir(Path::new(dir))?;
        }

        // 3. Verify via a query call and display the canister state.
        let shown = canister_call(&CanisterCallRequest {
            method: "show".to_string(),
            arg: "()".to_string(),
            call_type: Some(icp::sync_plugin::types::CallType::Query),
        })?;
        log(&format!("show() = {}", shown.trim()));

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
            let path_str = path.to_string_lossy();
            canister_call(&CanisterCallRequest {
                method: "register".to_string(),
                arg: format!(
                    "(\"{}\", \"{}\")",
                    escape_candid_text(&path_str),
                    escape_candid_text(content.trim())
                ),
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            log(&format!("{path_str}: ok"));
            count += 1;
        }
    }
    Ok(count)
}

fn escape_candid_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

export!(Plugin);
