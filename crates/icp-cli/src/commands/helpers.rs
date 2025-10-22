use candid::Principal;
/// Some helper functions to reduce boilerplate
use ic_agent::Agent;

use crate::{commands::{args::{ArgValidationError, Environment, Network}, Context}, options::IdentityOpt, store_id::Key};


pub(crate) async fn get_agent_for_env(ctx: &Context, identity: &IdentityOpt, environment: &Environment) -> Result<Agent, ArgValidationError> {

    // Get the environment name
    let ename = match environment {
        Environment::Name(name) => name.clone(),
        Environment::Default(name) => name.clone(),
    };

    // Load project
    let p = ctx.project.load().await?;

    // Load target environment
    let env =
        p.environments
            .get(&ename)
            .ok_or(ArgValidationError::EnvironmentNotFound {
                name: ename.to_owned(),
            })?;

    // Load identity
    let id = ctx.identity.load(identity.clone().into()).await?;

    // Access network
    let access = ctx.network.access(&env.network).await?;

    // Agent
    let agent = ctx.agent.create(id, &access.url).await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

    Ok(agent)

}

pub(crate) async fn get_agent_for_network(ctx: &Context, identity: &IdentityOpt, network: &Network) -> Result<Agent, ArgValidationError> {
    match network {
        Network::Name(nname) => {

            let p = ctx.project.load().await?;

            let network = p.networks.get(nname).ok_or(
                ArgValidationError::NetworkNotFound { name: nname.to_string() }
                )?;

            // Load identity
            let id = ctx.identity.load(identity.clone().into()).await?;

            // Access network
            let access = ctx.network.access(network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            Ok(agent)
        },
        Network::Url(url) => {

            let id = ctx.identity.load(identity.clone().into()).await?;

            // Agent
            let agent = ctx.agent.create(id, url).await?;

            Ok(agent)
        },
    }

}

pub(crate) async fn get_canister_id_for_env(ctx: &Context, cname: &String, environment: &Environment) -> Result<Principal, ArgValidationError> {

    // Get the environment name
    let ename = match environment {
        Environment::Name(name) => name.clone(),
        Environment::Default(name) => name.clone(),
    };

    // Load project
    let p = ctx.project.load().await?;

    // Load target environment
    let env =
        p.environments
            .get(&ename)
            .ok_or(ArgValidationError::EnvironmentNotFound {
                name: ename.to_owned(),
            })?;

    if !env.canisters.contains_key(cname) {
        return Err(ArgValidationError::CanisterNotInEnvironment {
            environment: env.name.to_owned(),
            canister: cname.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.ids.lookup(&Key {
        network: env.network.name.to_owned(),
        environment: env.name.to_owned(),
        canister: cname.to_owned(),
    })?;

    Ok(cid)
}
