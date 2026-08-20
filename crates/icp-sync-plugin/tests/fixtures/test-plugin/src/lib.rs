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
            "fields" => {
                if !input.fields.iter().any(|f| f.name == "greeting") {
                    return Err("missing 'greeting' field".to_string());
                }
                // Echo the fields back so the host can assert on what arrived.
                let rendered = input
                    .fields
                    .iter()
                    .map(|f| format!("{}={}", f.name, f.value))
                    .collect::<Vec<_>>()
                    .join(",");
                eprintln!("{rendered}");
                Ok(())
            }
            // Echo each dir/file entry as `kind key=path`, using "-" for an
            // absent key, so the host can assert keys survive the boundary.
            "keys" => {
                for dir in &input.dirs {
                    eprintln!("dir {}={}", dir.key.as_deref().unwrap_or("-"), dir.path);
                }
                for file in &input.files {
                    eprintln!("file {}={}", file.key.as_deref().unwrap_or("-"), file.name);
                }
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
