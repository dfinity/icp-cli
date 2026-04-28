#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../sync-plugin.wit",
});

struct TestPlugin;

impl Guest for TestPlugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        match input.environment.as_str() {
            "error" => Err("deliberate failure".to_string()),
            "hello" => Ok(Some("hello".to_string())),
            "print" => {
                println!("stdout from plugin");
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

export!(TestPlugin);
