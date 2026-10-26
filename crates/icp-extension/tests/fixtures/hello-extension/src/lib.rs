//! A test extension exercising every part of the `icp:extension` interface.

wit_bindgen::generate!({
    world: "extension",
    path: "../../../extension.wit",
});

use icp::extension::types::*;

struct Hello;

fn arg(name: &str, long: Option<&str>, kind: ArgKind, help: &str) -> Arg {
    Arg {
        name: name.into(),
        long: long.map(Into::into),
        short: None,
        kind,
        required: false,
        default_value: None,
        possible_values: vec![],
        value_name: None,
        help: help.into(),
    }
}

fn operand(name: &str, help: &str) -> Arg {
    Arg {
        required: true,
        ..arg(name, None, ArgKind::Value, help)
    }
}

fn command(path: &[&str], about: &str, args: Vec<Arg>, standard: Vec<StandardOpt>) -> Command {
    Command {
        path: path.iter().map(|s| s.to_string()).collect(),
        about: about.into(),
        args,
        standard,
        default_network: None,
    }
}

fn value<'a>(cmd: &'a Matched, name: &str) -> Option<&'a [String]> {
    cmd.values
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.values.as_slice())
}

// The empty Candid argument list: "DIDL", no types, no values.
const EMPTY_ARGS: &[u8] = b"DIDL\x00\x00";

impl Guest for Hello {
    fn describe() -> Info {
        let mut name = arg("name", None, ArgKind::Value, "Who to greet");
        name.default_value = Some("world".into());
        Info {
            name: "hello".into(),
            version: "0.1.0".into(),
            summary: "A test extension".into(),
            commands: vec![
                command(
                    &["greet"],
                    "Print a greeting",
                    vec![name, arg("shout", Some("shout"), ArgKind::Flag, "Shout it")],
                    vec![],
                ),
                command(
                    &["whoami"],
                    "Print what the host resolved",
                    vec![],
                    vec![StandardOpt::Identity, StandardOpt::Network],
                ),
                command(
                    &["call"],
                    "Query a canister method that takes no arguments",
                    vec![operand("canister", "Canister ID"), operand("method", "Method name")],
                    vec![StandardOpt::Network],
                ),
                command(&["ask"], "Confirm, then open a URL", vec![], vec![]),
                command(&["fail"], "Fail on purpose", vec![], vec![]),
            ],
            canisters: vec![AllowedCanister {
                id: "ryjl3-tyaaa-aaaaa-aaaba-cai".into(),
                label: "ICP ledger".into(),
            }],
        }
    }

    fn run(cmd: Matched, ctx: Context) -> Result<(), String> {
        match cmd.path.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["greet"] => {
                let name = value(&cmd, "name").and_then(|v| v.first()).unwrap();
                let greeting = format!("hello, {name}");
                if value(&cmd, "shout").is_some() {
                    println!("{}!", greeting.to_uppercase());
                } else {
                    println!("{greeting}");
                }
                Ok(())
            }
            ["whoami"] => {
                println!(
                    "identity={} network={} mainnet={} environment={} interactive={}",
                    ctx.identity_principal.as_deref().unwrap_or("-"),
                    ctx.network.as_deref().unwrap_or("-"),
                    ctx.mainnet,
                    ctx.environment.as_deref().unwrap_or("-"),
                    ctx.interactive,
                );
                for entry in &ctx.canister_ids {
                    println!("canister {}={}", entry.name, entry.id);
                }
                Ok(())
            }
            ["call"] => {
                let canister = value(&cmd, "canister").and_then(|v| v.first()).unwrap();
                let method = value(&cmd, "method").and_then(|v| v.first()).unwrap();
                let reply = canister_call(&CanisterCallRequest {
                    canister: canister.clone(),
                    method: method.clone(),
                    arg: EMPTY_ARGS.to_vec(),
                    call_type: CallType::Query,
                })?;
                println!("reply {} bytes", reply.len());
                Ok(())
            }
            ["ask"] => {
                if confirm("Open the page?", false) {
                    open_url("https://example.com/");
                    println!("opened");
                } else {
                    println!("declined");
                }
                Ok(())
            }
            ["fail"] => Err("deliberate failure".into()),
            other => Err(format!("unknown command {other:?}")),
        }
    }
}

export!(Hello);
