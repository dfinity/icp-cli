#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../sync-plugin.wit",
});

struct TestPlugin;

impl Guest for TestPlugin {
    fn exec(input: SyncExecInput) -> Result<(), String> {
        match input.environment.as_str() {
            "error" => Err("deliberate failure".to_string()),
            "hello" => {
                eprintln!("hello");
                Ok(())
            }
            "print" => {
                println!("stdout from plugin");
                Ok(())
            }
            "spin" => {
                // Busy-loop forever to exercise the host's compute-time limit.
                // The epoch-interruption check at the loop back-edge traps this,
                // so it never returns. `black_box` keeps the loop from being
                // optimized away.
                let mut x: u64 = 0;
                loop {
                    x = x.wrapping_add(1);
                    std::hint::black_box(x);
                }
            }
            _ => Ok(()),
        }
    }
}

export!(TestPlugin);
