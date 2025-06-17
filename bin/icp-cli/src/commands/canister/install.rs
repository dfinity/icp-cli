use crate::env::Env;
use clap::Parser;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInstallCmd {}

pub async fn exec(_env: &Env, _cmd: CanisterInstallCmd) -> Result<(), CanisterInstallError> {
    // // Install
    // mgmt.install_code(&canister_id, &wasm_module)
    //     .with_mode(InstallMode::Install)
    //     .await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterInstallError {
    #[snafu(display("{error}"))]
    Unexpected { error: String },
}
