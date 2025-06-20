use crate::env::Env;
use clap::Parser;
use icp_identity::manifest::{
    IdentityDefaults, IdentityList, load_identity_defaults, load_identity_list,
};
use snafu::Snafu;

use crate::options::{Format, WithFormat};

#[derive(Debug, Parser)]
pub struct ListCmd {
    #[command(flatten)]
    with_format: WithFormat,
}

pub fn exec(env: &Env, cmd: ListCmd) -> Result<(), ListKeysError> {
    let dirs = env.dirs();
    let list = load_identity_list(dirs)?;
    let defaults = load_identity_defaults(dirs)?;

    match cmd.with_format.format {
        Format::Json => handle_json_output(list, defaults),
        Format::Text => handle_text_output(list, defaults),
    }

    Ok(())
}

// Prints out the JSON output
// TODO: This needs to flag the default identity
fn handle_json_output(list: IdentityList, _defaults: IdentityDefaults) {
    println!(
        "{}",
        serde_json::to_string(&list).expect("Serializing the identities to JSON should never fail")
    );
}

fn handle_text_output(list: IdentityList, defaults: IdentityDefaults) {
    for id in list.identities.keys() {
        if *id == defaults.default {
            println!("* {id}");
        } else {
            println!("  {id}");
        }
    }
}

#[derive(Debug, Snafu)]
pub enum ListKeysError {
    #[snafu(transparent)]
    LoadIdentity {
        source: icp_identity::manifest::LoadIdentityManifestError,
    },
}
