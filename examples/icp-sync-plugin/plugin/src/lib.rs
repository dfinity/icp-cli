wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../crates/icp-sync-plugin/sync-plugin.wit",
});

use std::fs;
use std::path::Path;

use candid::{Encode, Principal};

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        println!(
            "sync plugin: starting for canister {} (environment: {})",
            input.canister_id, input.environment
        );

        // 1. Set the uploader to the current identity principal.
        //    Routed through the proxy (direct: false) so the controller-gated
        //    call is signed by the proxy canister, which is a controller.
        let uploader = Principal::from_text(&input.identity_principal)
            .map_err(|e| format!("invalid identity principal: {e}"))?;
        let arg = Encode!(&uploader).map_err(|e| format!("encode set_uploader arg: {e}"))?;
        canister_call(&CanisterCallRequest {
            method: "set_uploader".to_string(),
            arg,
            call_type: icp::sync_plugin::types::CallType::Update,
            direct: false,
            cycles: 0,
        })?;
        println!("set_uploader ({}): ok", input.identity_principal);

        // 2. Register every file found by traversing the preopened dirs.
        //    Direct calls (direct: true) because register is gated on the
        //    uploader principal, which is the current identity — not the proxy.
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
                call_type: icp::sync_plugin::types::CallType::Update,
                direct: true,
                cycles: 0,
            })?;
            println!("{path_str}: ok");
            count += 1;
        }
    }
    Ok(count)
}

export!(Plugin);
