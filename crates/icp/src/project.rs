use std::collections::{HashMap, HashSet, hash_map::Entry};

use snafu::prelude::*;

use crate::{
    Canister, Environment, Network, Project,
    canister::recipe,
    manifest::{
        CANISTER_MANIFEST, CanisterManifest, EnvironmentManifest, Item, LoadManifestFromPathError,
        NetworkManifest, ProjectManifest, ProjectRootLocateError,
        canister::{Instructions, SyncSteps},
        environment::CanisterSelection,
        load_manifest_from_path,
        recipe::RecipeType,
    },
    network::{Configuration, Connected, Gateway, Managed, ManagedMode, Port},
    prelude::*,
};

pub const DEFAULT_LOCAL_ENVIRONMENT_NAME: &str = "local";
pub const DEFAULT_MAINNET_ENVIRONMENT_NAME: &str = "ic";
pub const DEFAULT_LOCAL_NETWORK_NAME: &str = "local";
pub const DEFAULT_MAINNET_NETWORK_NAME: &str = "mainnet";
pub const DEFAULT_LOCAL_NETWORK_HOST: &str = "localhost";
pub const DEFAULT_LOCAL_NETWORK_PORT: u16 = 8000;
pub const DEFAULT_LOCAL_NETWORK_URL: &str = "http://localhost:8000";
pub const DEFAULT_MAINNET_NETWORK_URL: &str = IC_MAINNET_NETWORK_URL;

#[derive(Debug, Snafu)]
pub enum EnvironmentError {
    #[snafu(display("environment '{environment}' points to invalid network '{network}'"))]
    InvalidNetwork {
        environment: String,
        network: String,
    },

    #[snafu(display("environment '{environment}' points to invalid canister '{canister}'"))]
    InvalidCanister {
        environment: String,
        canister: String,
    },
}

#[derive(Debug, Snafu)]
pub enum ConsolidateManifestError {
    #[snafu(display("failed to locate project directory"))]
    Locate { source: ProjectRootLocateError },

    #[snafu(display("failed to perform glob parsing"))]
    GlobParse { source: glob::PatternError },

    #[snafu(display("failed to get glob iter"))]
    GlobIter { source: glob::GlobError },

    #[snafu(display("failed to convert path to UTF-8"))]
    Utf8Path { source: FromPathBufError },

    #[snafu(display("failed to load canister manifest"))]
    LoadCanister { source: LoadManifestFromPathError },

    #[snafu(display("failed to load network manifest"))]
    LoadNetwork { source: LoadManifestFromPathError },

    #[snafu(display("failed to load environment manifest"))]
    LoadEnvironment { source: LoadManifestFromPathError },

    #[snafu(display("failed to load {kind} manifest at: {path}"))]
    Failed { kind: String, path: String },

    #[snafu(display("failed to resolve canister recipe: {recipe_type:?}"))]
    Recipe {
        source: recipe::ResolveError,
        recipe_type: RecipeType,
    },

    #[snafu(display("project contains two similarly named {kind}s: '{name}'"))]
    Duplicate { kind: String, name: String },

    #[snafu(display("`{name}` is a reserved {kind} name."))]
    Reserved { kind: String, name: String },

    #[snafu(display("could not locate a {kind} manifest at: '{path}'"))]
    NotFound { kind: String, path: String },

    #[snafu(transparent)]
    Environment { source: EnvironmentError },
}

/// Returns the default mainnet network (protected, non-overridable)
fn default_mainnet_network() -> Network {
    Network {
        // Mainnet at https://icp-api.io
        name: DEFAULT_MAINNET_NETWORK_NAME.to_string(),
        configuration: Configuration::Connected {
            connected: Connected {
                url: IC_MAINNET_NETWORK_URL.to_string(),
                // Will use the IC Root key hard coded in agent-rs.
                // https://github.com/dfinity/agent-rs/blob/b77f1fc5fe05d8de1065ee4cec837bc3f2ce9976/ic-agent/src/agent/mod.rs#L82
                root_key: None,
            },
        },
    }
}

/// Returns the default local network (can be overridden by users)
fn default_local_network() -> Network {
    Network {
        // The local network at localhost:8000
        name: DEFAULT_LOCAL_NETWORK_NAME.to_string(),
        configuration: Configuration::Managed {
            managed: Managed {
                mode: ManagedMode::Launcher {
                    gateway: Gateway {
                        host: DEFAULT_LOCAL_NETWORK_HOST.to_string(),
                        port: Port::Fixed(DEFAULT_LOCAL_NETWORK_PORT),
                    },
                },
            },
        },
    }
}

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

/// Turns the ProjectManifest into a Project struct
/// - Adds the default Networks
/// - Adds the default Environment
/// - Validates the manifest to make sure that:
///     - There are no duplicates
///     - All the environments have networks
///     - All the referenced canisters exist
///     - All the recipes have been resolved
pub async fn consolidate_manifest(
    pdir: &Path,
    recipe_resolver: &dyn recipe::Resolve,
    m: &ProjectManifest,
) -> Result<Project, ConsolidateManifestError> {
    // Canisters
    let mut canisters: HashMap<String, (PathBuf, Canister)> = HashMap::new();

    for i in &m.canisters {
        let ms = match i {
            Item::Path(pattern) => {
                let is_glob_pattern = is_glob(pattern);
                let paths = match is_glob_pattern {
                    // Explicit path
                    false => vec![pdir.join(pattern)],

                    // Glob pattern
                    true => {
                        // Resolve glob
                        let paths =
                            glob::glob(pdir.join(pattern).as_str()).context(GlobParseSnafu)?;

                        let mut v = vec![];
                        for p in paths {
                            let path = p.context(GlobIterSnafu)?;
                            let utf8_path = PathBuf::try_from(path).context(Utf8PathSnafu)?;
                            v.push(utf8_path);
                        }
                        v
                    }
                };

                let paths = if is_glob_pattern {
                    // For glob patterns, filter out non-directories and non-canister directories
                    paths
                        .into_iter()
                        .filter(|p| p.is_dir())
                        .filter(|p| p.join(CANISTER_MANIFEST).exists())
                        .collect::<Vec<_>>()
                } else {
                    // For explicit paths, validate that they exist and contain canister.yaml
                    let mut validated_paths = vec![];
                    for p in paths {
                        if !p.is_dir() {
                            return NotFoundSnafu {
                                kind: "canister".to_string(),
                                path: pattern.to_string(),
                            }
                            .fail();
                        }
                        if !p.join(CANISTER_MANIFEST).exists() {
                            return NotFoundSnafu {
                                kind: "canister".to_string(),
                                path: pattern.to_string(),
                            }
                            .fail();
                        }
                        validated_paths.push(p);
                    }
                    validated_paths
                };

                let mut ms = vec![];

                for p in paths {
                    ms.push((
                        //
                        // Canister root
                        p.to_owned(),
                        //
                        // Canister manifest
                        load_manifest_from_path::<CanisterManifest>(&p.join(CANISTER_MANIFEST))
                            .await
                            .context(LoadCanisterSnafu)?,
                    ));
                }

                ms
            }

            Item::Manifest(m) => vec![(
                //
                // Canister root
                pdir.to_owned(),
                //
                // Canister manifest
                m.to_owned(),
            )],
        };

        for (cdir, m) in ms {
            let (build, sync) = match &m.instructions {
                // Build/Sync
                Instructions::BuildSync { build, sync } => (
                    build.to_owned(),
                    match sync {
                        Some(sync) => sync.to_owned(),
                        None => SyncSteps::default(),
                    },
                ),

                // Recipe
                Instructions::Recipe { recipe } => {
                    recipe_resolver.resolve(recipe).await.context(RecipeSnafu {
                        recipe_type: recipe.recipe_type.clone(),
                    })?
                }
            };

            // Check for duplicates
            match canisters.entry(m.name.to_owned()) {
                // Duplicate
                Entry::Occupied(e) => {
                    return DuplicateSnafu {
                        kind: "canister".to_string(),
                        name: e.key().to_owned(),
                    }
                    .fail();
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert((
                        //
                        // Canister root
                        cdir,
                        //
                        // Canister
                        Canister {
                            name: m.name.to_owned(),
                            settings: m.settings.to_owned(),
                            build,
                            sync,
                            init_args: m.init_args.to_owned(),
                        },
                    ));
                }
            }
        }
    }

    // Networks
    let mut networks: HashMap<String, Network> = HashMap::new();

    // Add mainnet first - this is always protected and non-overridable
    networks.insert(
        DEFAULT_MAINNET_NETWORK_NAME.to_string(),
        default_mainnet_network(),
    );

    // Track which network names are protected (only mainnet)
    let protected_network_names: HashSet<String> = [DEFAULT_MAINNET_NETWORK_NAME.to_string()]
        .into_iter()
        .collect();

    // Resolve NetworkManifests and add them (including user-defined "local" if provided)
    for i in &m.networks {
        let m = match i {
            Item::Path(path) => {
                let path = pdir.join(path);
                if !path.exists() || !path.is_file() {
                    return NotFoundSnafu {
                        kind: "network".to_string(),
                        path: path.to_string(),
                    }
                    .fail();
                }
                load_manifest_from_path::<NetworkManifest>(&path)
                    .await
                    .context(LoadNetworkSnafu)?
            }
            Item::Manifest(ms) => ms.clone(),
        };

        match networks.entry(m.name.to_owned()) {
            // Duplicate
            Entry::Occupied(e) => {
                // Only error if trying to override a protected network
                if protected_network_names.contains(&m.name) {
                    return ReservedSnafu {
                        kind: "network".to_string(),
                        name: m.name.to_string(),
                    }
                    .fail();
                }

                // For non-protected duplicates, this is a user error (defining same network twice)
                return DuplicateSnafu {
                    kind: "network".to_string(),
                    name: e.key().to_owned(),
                }
                .fail();
            }

            // Ok
            Entry::Vacant(e) => {
                e.insert(Network {
                    name: m.name.to_owned(),
                    configuration: m.configuration.into(), // Convert manifest to config struct
                });
            }
        }
    }

    // After processing user networks, add default "local" if not already defined
    // This provides backward compatibility for projects that don't define their own "local" network
    if !networks.contains_key(DEFAULT_LOCAL_NETWORK_NAME) {
        networks.insert(
            DEFAULT_LOCAL_NETWORK_NAME.to_string(),
            default_local_network(),
        );
    }

    // Environments
    let mut environments: HashMap<String, Environment> = HashMap::new();

    for i in &m.environments {
        let m = match i {
            Item::Path(path) => {
                let path = pdir.join(path);
                if !path.exists() || !path.is_file() {
                    return NotFoundSnafu {
                        kind: "environment".to_string(),
                        path: path.to_string(),
                    }
                    .fail();
                }
                load_manifest_from_path::<EnvironmentManifest>(&path)
                    .await
                    .context(LoadEnvironmentSnafu)?
            }
            Item::Manifest(ms) => ms.clone(),
        };

        match environments.entry(m.name.to_owned()) {
            // Duplicate
            Entry::Occupied(e) => {
                return DuplicateSnafu {
                    kind: "environment".to_string(),
                    name: e.key().to_owned(),
                }
                .fail();
            }

            // Ok
            Entry::Vacant(e) => {
                e.insert(Environment {
                    name: m.name.to_owned(),

                    // Embed network in environment
                    network: {
                        let v = networks.get(&m.network).ok_or(
                            InvalidNetworkSnafu {
                                environment: m.name.to_owned(),
                                network: m.network.to_owned(),
                            }
                            .build(),
                        )?;

                        v.to_owned()
                    },

                    // Embed canisters in environment
                    canisters: {
                        let mut cs = match &m.canisters {
                            // None
                            CanisterSelection::None => HashMap::new(),

                            // Everything
                            CanisterSelection::Everything => canisters.clone(),

                            // Named
                            CanisterSelection::Named(names) => {
                                let mut cs: HashMap<String, (PathBuf, Canister)> = HashMap::new();

                                for name in names {
                                    let v = canisters.get(name).ok_or(
                                        InvalidCanisterSnafu {
                                            environment: m.name.to_owned(),
                                            canister: name.to_owned(),
                                        }
                                        .build(),
                                    )?;

                                    cs.insert(name.to_owned(), v.to_owned());
                                }

                                cs
                            }
                        };

                        // Apply settings overrides if specified
                        if let Some(ref settings_overrides) = m.settings {
                            for (canister_name, settings) in settings_overrides {
                                if let Some((_path, canister)) = cs.get_mut(canister_name) {
                                    canister.settings = settings.clone();
                                }
                            }
                        }

                        // Apply init_args overrides if specified
                        if let Some(ref init_args_overrides) = m.init_args {
                            for (canister_name, init_args) in init_args_overrides {
                                if let Some((_path, canister)) = cs.get_mut(canister_name) {
                                    canister.init_args = Some(init_args.clone());
                                }
                            }
                        }

                        cs
                    },
                });
            }
        }
    }

    // We're done adding all the user environments
    // Now we add the default `local` environment if the user hasn't overriden it
    if let Entry::Vacant(vacant_entry) =
        environments.entry(DEFAULT_LOCAL_ENVIRONMENT_NAME.to_string())
    {
        vacant_entry.insert(Environment {
            name: DEFAULT_LOCAL_ENVIRONMENT_NAME.to_string(),
            network: networks
                .get(DEFAULT_LOCAL_NETWORK_NAME)
                .ok_or(
                    InvalidNetworkSnafu {
                        environment: DEFAULT_LOCAL_ENVIRONMENT_NAME.to_owned(),
                        network: DEFAULT_LOCAL_NETWORK_NAME.to_owned(),
                    }
                    .build(),
                )?
                .to_owned(),
            canisters: canisters.clone(),
        });
    }

    Ok(Project {
        dir: pdir.into(),
        canisters,
        networks,
        environments,
    })
}
