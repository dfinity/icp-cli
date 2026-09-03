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
            // Echo each entry as `kind key=path`, so the host can assert that
            // keys survive the boundary and that a `files:` entry lands in the
            // list its kind on disk calls for.
            "keys" => {
                for dir in &input.dirs {
                    eprintln!("dir {}={}", dir.key, dir.path);
                }
                for file in &input.files {
                    eprintln!("file {}={}", file.key, file.name);
                }
                Ok(())
            }
            // List each declared dir as `key=entry,entry`, so the host can
            // assert a dir reached only through a preopened ancestor is still
            // readable.
            "read-dirs" => {
                for dir in &input.dirs {
                    let mut names = std::fs::read_dir(&dir.path)
                        .and_then(|entries| {
                            entries
                                .map(|entry| {
                                    entry.map(|e| e.file_name().to_string_lossy().into_owned())
                                })
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .map_err(|err| format!("reading '{}': {err}", dir.path))?;
                    names.sort();
                    eprintln!("{}={}", dir.key, names.join(","));
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
