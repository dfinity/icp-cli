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
            // Echo the network URLs back as `api=<url> gateway=<url>`, using
            // "-" for a network with no gateway.
            "urls" => {
                eprintln!(
                    "api={} gateway={}",
                    input.api_url,
                    input.gateway_url.as_deref().unwrap_or("-")
                );
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
            // Ask for a metadata section from a canister the step did not
            // declare. The host must reject the target before it touches the
            // network, so this needs no live canister; echo the refusal.
            "metadata-undeclared" => {
                let err = canister_metadata_section(&MetadataSectionRequest {
                    target: CallTarget::Name("undeclared".to_string()),
                    name: "candid:service".to_string(),
                    direct: true,
                })
                .expect_err("host must reject an undeclared target");
                eprintln!("{err}");
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
            // Wait on the host for an hour. The wait is a WASI poll, during
            // which no wasm runs, so only a wall-clock limit can end it.
            "sleep" => {
                std::thread::sleep(std::time::Duration::from_secs(3600));
                Ok(())
            }
            // Make one update call to the canister being synced, and report
            // how it went. Used with a host whose call never returns, to
            // exercise the wall-clock limit cutting off a call in flight.
            "call-host" => {
                let reply = canister_call(&CanisterCallRequest {
                    target: CallTarget::Host,
                    method: "hang".to_string(),
                    arg: vec![],
                    call_type: icp::sync_plugin::types::CallType::Update,
                    direct: true,
                    cycles: 0,
                })?;
                eprintln!("reply of {} bytes", reply.len());
                Ok(())
            }
            // Take 256 MiB of memory and touch all of it, to exercise the
            // host's memory limit.
            "grow" => {
                let buf = std::hint::black_box(vec![1u8; 256 << 20]);
                eprintln!("held {} bytes", buf.iter().map(|&b| b as usize).sum::<usize>());
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

export!(TestPlugin);
