use candid::Principal;
use icp::{
    context::{Context, EnvironmentSelection},
    identity::IdentitySelection,
};

use crate::commands::canister::create::CanisterSettings;

pub(crate) struct CreateOperation<'a> {
    ctx: &'a Context,
    canisters: Vec<String>,
    environment: &'a EnvironmentSelection,
    identity: &'a IdentitySelection,
    subnet: Option<Principal>,
    controllers: Vec<Principal>,
    cycles: u128,
    settings: CanisterSettings,
}

impl<'a> CreateOperation<'a> {
    pub(crate) fn new(
        ctx: &'a Context,
        canisters: Vec<String>,
        environment: &'a EnvironmentSelection,
        identity: &'a IdentitySelection,
        subnet: Option<Principal>,
        controllers: Vec<Principal>,
        cycles: u128,
        settings: CanisterSettings,
    ) -> Self {
        Self {
            ctx,
            canisters,
            environment,
            identity,
            subnet,
            controllers,
            cycles,
            settings,
        }
    }
}
