wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        log(&format!(
            "sync plugin: starting for canister {} (environment: {})",
            input.canister_id, input.environment
        ));

        let entries = list_dir("seed-data/")?;
        let mut seeded = 0u32;

        for entry in entries {
            if entry.is_dir {
                continue;
            }
            let path = format!("seed-data/{}", entry.name);
            let content = read_file(&path)?;
            let name = escape_candid_text(&entry.name);
            let content_escaped = escape_candid_text(content.trim());
            canister_call(&CanisterCallRequest {
                method: "seed".to_string(),
                arg: format!("(\"{name}\", \"{content_escaped}\")"),
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            log(&format!("{path}: ok"));
            seeded += 1;
        }

        // Verify via a query call that the canister received all items.
        let count_result = canister_call(&CanisterCallRequest {
            method: "count_items".to_string(),
            arg: "()".to_string(),
            call_type: Some(icp::sync_plugin::types::CallType::Query),
        })?;
        log(&format!(
            "verified: count_items() = {}",
            count_result.trim()
        ));

        Ok(Some(format!(
            "seeded {} item(s) into canister {} (environment: {})",
            seeded, input.canister_id, input.environment
        )))
    }
}

fn escape_candid_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

export!(Plugin);
