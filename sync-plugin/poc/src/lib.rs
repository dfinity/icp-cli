wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        log(&format!(
            "sync plugin: starting for canister {}",
            input.canister_id
        ));

        let entries = list_dir("seed-data/")?;

        for entry in entries {
            if entry.is_dir {
                continue;
            }
            let path = format!("seed-data/{}", entry.name);
            let data = read_file(&path)?;
            canister_call(&CanisterCallRequest {
                method: "seed".to_string(),
                arg: format!(
                    "(\"{}\")",
                    data.trim().replace('\\', "\\\\").replace('"', "\\\"")
                ),
                call_type: Some(icp::sync_plugin::types::CallType::Update),
            })?;
            log(&format!("{path}: ok"));
        }

        Ok(Some(format!(
            "seeded canister {} in environment {}",
            input.canister_id, input.environment
        )))
    }
}

export!(Plugin);
