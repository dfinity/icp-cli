wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../crates/icp-sync-plugin/sync-plugin.wit",
});

use std::fs;
use std::path::Path;

use candid::{Encode, Principal};

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<(), String> {
        println!(
            "sync plugin: starting for canister {} (environment: {})",
            input.canister_id, input.environment
        );

        // 1. Report the canister's Candid interface, read from its metadata.
        //    Reported rather than required: the section is only there if the
        //    build embedded it (this project's build does, via ic-wasm).
        let interface = canister_metadata_section(&MetadataSectionRequest {
            target: CallTarget::Host,
            name: "candid:service".to_string(),
            direct: false,
        })?;
        match &interface {
            Some(bytes) => eprintln!("candid:service: {} bytes", bytes.len()),
            None => eprintln!("candid:service: absent"),
        }

        // 2. Set the uploader to the current identity principal.
        //    Routed through the proxy (direct: false) so the controller-gated
        //    call is signed by the proxy canister, which is a controller.
        let uploader = Principal::from_text(&input.identity_principal)
            .map_err(|e| format!("invalid identity principal: {e}"))?;
        let arg = Encode!(&uploader).map_err(|e| format!("encode set_uploader arg: {e}"))?;
        canister_call(&CanisterCallRequest {
            target: CallTarget::Host,
            method: "set_uploader".to_string(),
            arg,
            call_type: icp::sync_plugin::types::CallType::Update,
            direct: false,
            cycles: 0,
        })?;
        println!("set_uploader ({}): ok", input.identity_principal);

        // 3. Record which environment seeded the canister as an environment
        //    variable, so the canister's own code can read it back. Setting
        //    settings is controller-gated like set_uploader, so it takes the
        //    same route (direct: false).
        canister_set_environment_variable(&SetEnvironmentVariableRequest {
            target: CallTarget::Host,
            name: "SEEDED_BY".to_string(),
            value: input.environment.clone(),
            direct: false,
        })?;
        eprintln!("SEEDED_BY={}", input.environment);

        // 4. Register every file found by traversing the preopened dirs.
        //    Direct calls (direct: true) because register is gated on the
        //    uploader principal, which is the current identity — not the proxy.
        let mut registered = 0u32;
        for dir in &input.dirs {
            registered += register_dir(Path::new(&dir.path))?;
        }

        // Persisted after the step completes; use stderr.
        eprintln!(
            "registered {} item(s) in canister {} (environment: {})",
            registered, input.canister_id, input.environment
        );
        Ok(())
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
                target: CallTarget::Host,
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
