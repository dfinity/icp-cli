#![allow(clippy::too_many_arguments)]

// A plugin built against the *legacy* (v0.1.0) interface, used to prove the host
// still loads and drives v0.1.0 plugins alongside v0.2.0 ones. It cannot choose
// a call target and never sees the canister ID table — that is the whole point.

wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../sync-plugin-v1.wit",
});

struct TestPluginV1;

impl Guest for TestPluginV1 {
    fn exec(input: SyncExecInput) -> Result<(), String> {
        match input.environment.as_str() {
            "error" => Err("deliberate v1 failure".to_string()),
            "hello" => {
                eprintln!("hello from v1");
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

export!(TestPluginV1);
