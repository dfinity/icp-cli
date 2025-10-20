use crate::commands::{
    Mode,
    args::{Canister, Environment, Network},
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ValidateError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) trait Validate {
    fn validate(&self, mode: &Mode) -> Result<(), ValidateError>;
}

#[cfg(test)]
pub(crate) mod helpers {
    use crate::commands::Mode;

    // #[cfg(test)]
    // pub(crate) trait IntoOptions<T> {
    //     fn into_options(self) -> Vec<Option<T>>;
    // }

    // #[cfg(test)]
    // impl<T> IntoOptions<T> for Vec<T> {
    //     fn into_options(self) -> Vec<Option<T>> {
    //         self.into_iter().fold(vec![None], |mut acc, cur| {
    //             acc.push(Some(cur));
    //             acc
    //         })
    //     }
    // }

    pub(crate) fn all_modes() -> Vec<Mode> {
        vec![Mode::Global, Mode::Project("dir".into())]
    }

    // pub(crate) fn all_networks() -> Vec<args::Network> {
    //     vec![
    //         args::Network::Name("my-network".to_string()),
    //         args::Network::Url("http::/www.example.com".to_string()),
    //     ]
    // }
}

pub(crate) fn a_canister_id_is_required_in_global_mode<'a>(
    canister: impl Into<(&'a Canister,)>,
    m: &Mode,
) -> Option<&'static str> {
    let (canister,) = canister.into();
    (matches!(m, Mode::Global) && !matches!(canister, Canister::Principal(_)))
        .then_some(PLEASE_PROVIDE_A_CANISTER_PRINCIPAL_IN_GLOBAL_MODE)
}

const PLEASE_PROVIDE_A_CANISTER_PRINCIPAL_IN_GLOBAL_MODE: &str = r#"
    Please provide a canister principal in global mode.
"#;

pub(crate) fn network_or_environment_not_both<'a>(
    network_environment: impl Into<(&'a Option<Network>, &'a Option<Environment>)>,
    m: &Mode,
) -> Option<&'static str> {
    let (network, environment) = network_environment.into();
    (matches!(m, _) && network.is_some() && environment.is_some())
        .then_some(PLEASE_PROVIDE_EITHER_A_NETWORK_OR_AN_ENVIRONMENT_BUT_NOT_BOTH)
}

const PLEASE_PROVIDE_EITHER_A_NETWORK_OR_AN_ENVIRONMENT_BUT_NOT_BOTH: &str = r#"
    Please provide either a network or an environment, but not both.
"#;

pub(crate) fn environments_are_not_available_in_a_global_mode<'a>(
    environment: impl Into<(&'a Option<Environment>,)>,
    m: &Mode,
) -> Option<&'static str> {
    let (environment,) = environment.into();
    (matches!(m, Mode::Global) && environment.is_some())
        .then_some(ENVIRONMENTS_ARE_NOT_AVAILABLE_IN_GLOBAL_MODE)
}

const ENVIRONMENTS_ARE_NOT_AVAILABLE_IN_GLOBAL_MODE: &str = r#"
    Environments are not available in global mode.
"#;

pub(crate) fn a_network_url_is_required_in_global_mode<'a>(
    network: impl Into<(&'a Option<Network>,)>,
    m: &Mode,
) -> Option<&'static str> {
    let (network,) = network.into();
    (matches!(m, Mode::Global) && !matches!(network, Some(Network::Url(_))))
        .then_some(A_NETWORK_URL_IS_REQUIRED_IN_GLOBAL_MODE)
}

const A_NETWORK_URL_IS_REQUIRED_IN_GLOBAL_MODE: &str = r#"
    A network `url` is required in global mode.
"#;

#[cfg(test)]
mod test_a_canister_id_is_required_in_global_mode {
    use crate::impl_from_args;

    use super::*;

    struct Args {
        canister: Canister,
    }

    impl_from_args!(Args, canister: Canister);

    #[test]
    fn test() {
        let out = a_canister_id_is_required_in_global_mode(
            //
            // Args
            &Args {
                canister: Canister::Name("my-canister".to_string()),
            },
            //
            // Mode
            &Mode::Global,
        );
        match out {
            Some(msg) if msg == PLEASE_PROVIDE_A_CANISTER_PRINCIPAL_IN_GLOBAL_MODE => {}
            _ => panic!("invalid validation output: {out:?}"),
        }
    }
}

#[cfg(test)]
mod test_network_or_environment_not_both {
    use crate::{
        commands::{args, validation},
        impl_from_args,
    };

    use super::*;

    struct Args {
        network: Option<args::Network>,
        environment: Option<args::Environment>,
    }

    impl_from_args!(Args, network: Option<args::Network>, environment: Option<args::Environment>);

    #[test]
    fn test() {
        for (args, modes) in [
            (
                //
                // Args
                &Args {
                    network: Some(args::Network::Name("my-network".to_string())),
                    environment: Some(args::Environment::Name("my-environment".to_string())),
                },
                //
                // Modes
                validation::helpers::all_modes(),
            ),
            (
                //
                // Args
                &Args {
                    network: Some(args::Network::Url("http://www.example.com".to_string())),
                    environment: Some(args::Environment::Name("my-environment".to_string())),
                },
                //
                // Modes
                validation::helpers::all_modes(),
            ),
        ] {
            for mode in &modes {
                let out = network_or_environment_not_both(args, mode);
                match out {
                    Some(msg)
                        if msg
                            == PLEASE_PROVIDE_EITHER_A_NETWORK_OR_AN_ENVIRONMENT_BUT_NOT_BOTH => {}
                    _ => panic!("invalid validation output: {out:?}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod test_environments_are_not_available_in_a_global_mode {
    use crate::{commands::args, impl_from_args};

    use super::*;

    struct Args {
        environment: Option<args::Environment>,
    }

    impl_from_args!(Args, environment: Option<args::Environment>);

    #[test]
    fn test() {
        let out = environments_are_not_available_in_a_global_mode(
            //
            // Args
            &Args {
                environment: Some(args::Environment::Name("my-environment".to_string())),
            },
            //
            // Mode
            &Mode::Global,
        );
        match out {
            Some(msg) if msg == ENVIRONMENTS_ARE_NOT_AVAILABLE_IN_GLOBAL_MODE => {}
            _ => panic!("invalid validation output: {out:?}"),
        }
    }
}

#[cfg(test)]
mod test_a_network_url_is_required_in_global_mode {
    use crate::{commands::args, impl_from_args};

    use super::*;

    struct Args {
        network: Option<args::Network>,
    }

    impl_from_args!(Args, network: Option<args::Network>);

    #[test]
    fn test() {
        let out = a_network_url_is_required_in_global_mode(
            //
            // Args
            &Args {
                network: Some(args::Network::Name("my-network".to_string())),
            },
            //
            // Mode
            &Mode::Global,
        );
        match out {
            Some(msg) if msg == A_NETWORK_URL_IS_REQUIRED_IN_GLOBAL_MODE => {}
            _ => panic!("invalid validation output: {out:?}"),
        }
    }
}
