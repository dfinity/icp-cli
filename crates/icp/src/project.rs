use std::collections::{HashMap, HashSet, hash_map::Entry};

use candid_parser::parse_idl_args;
use snafu::prelude::*;

use crate::{
    Canister, Environment, InitArgs, Network, Project,
    canister::recipe,
    fs,
    manifest::{
        CANISTER_MANIFEST, CanisterManifest, EnvironmentManifest, InitArgsFormat, Item,
        LoadManifestFromPathError, ManifestInitArgs, NetworkManifest, ProjectManifest,
        ProjectRootLocateError,
        canister::{Instructions, SyncSteps},
        environment::CanisterSelection,
        load_manifest_from_path,
        recipe::RecipeType,
    },
    network::{
        Configuration, Connected, Gateway, Managed, ManagedLauncherConfig, ManagedMode, Port,
    },
    prelude::*,
};

pub const DEFAULT_LOCAL_NETWORK_HOST: &str = "localhost";
pub const DEFAULT_LOCAL_NETWORK_PORT: u16 = 8000;
pub const DEFAULT_LOCAL_NETWORK_URL: &str = "http://localhost:8000";

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
        #[snafu(source(from(recipe::ResolveError, Box::new)))]
        source: Box<recipe::ResolveError>,
        recipe_type: RecipeType,
    },

    #[snafu(display("project contains two similarly named {kind}s: '{name}'"))]
    Duplicate { kind: String, name: String },

    #[snafu(display("`{name}` is a reserved {kind} name."))]
    Reserved { kind: String, name: String },

    #[snafu(display("could not locate a {kind} manifest at: '{path}'"))]
    NotFound { kind: String, path: String },

    #[snafu(display("failed to read init_args file for canister '{canister}'"))]
    ReadInitArgs {
        source: fs::IoError,
        canister: String,
    },

    #[snafu(display(
        "init_args file '{path}' for canister '{canister}' is neither valid UTF-8 text nor binary candid (DIDL)"
    ))]
    InvalidInitArgs { canister: String, path: PathBuf },

    #[snafu(display(
        "init_args '{value}' for canister '{canister}' is not valid hex, Candid, or a file path"
    ))]
    InitArgsNotFound {
        canister: String,
        value: String,
        path: PathBuf,
    },

    #[snafu(display(
        "init_args for canister '{canister}' uses format 'bin' with inline content; \
         binary format requires a file path"
    ))]
    BinFormatInlineContent { canister: String },

    #[snafu(transparent)]
    Environment { source: EnvironmentError },
}

/// Resolve a [`ManifestInitArgs`] into a canonical [`InitArgs`] by reading
/// any file references relative to `base_path`.
fn resolve_manifest_init_args(
    manifest_init_args: &ManifestInitArgs,
    base_path: &Path,
    canister: &str,
) -> Result<InitArgs, ConsolidateManifestError> {
    match manifest_init_args {
        // String form: detect format, fall back to file path if not valid content.
        ManifestInitArgs::Inline(s) => {
            let detected = InitArgs::detect(s.clone().into_bytes())
                .expect("inline string is always valid UTF-8");
            match &detected {
                InitArgs::Text {
                    format: InitArgsFormat::Hex,
                    ..
                } => Ok(detected),
                InitArgs::Text {
                    content,
                    format: InitArgsFormat::Idl,
                } => {
                    if parse_idl_args(content.trim()).is_ok() {
                        Ok(detected)
                    } else if base_path.join(s).is_file() {
                        resolve_manifest_init_args(
                            &ManifestInitArgs::Path {
                                path: s.clone(),
                                format: None,
                            },
                            base_path,
                            canister,
                        )
                    } else {
                        InitArgsNotFoundSnafu {
                            canister,
                            value: s,
                            path: base_path.join(s),
                        }
                        .fail()
                    }
                }
                _ => Ok(detected),
            }
        }

        // Explicit file reference.
        ManifestInitArgs::Path { path, format } => {
            let file_path = base_path.join(path);
            match format.as_ref() {
                Some(InitArgsFormat::Bin) => {
                    let bytes = fs::read(&file_path).context(ReadInitArgsSnafu { canister })?;
                    Ok(InitArgs::Binary(bytes))
                }
                Some(fmt) => {
                    let content =
                        fs::read_to_string(&file_path).context(ReadInitArgsSnafu { canister })?;
                    Ok(InitArgs::Text {
                        content: content.trim().to_owned(),
                        format: fmt.clone(),
                    })
                }
                None => {
                    let bytes = fs::read(&file_path).context(ReadInitArgsSnafu { canister })?;
                    InitArgs::detect(bytes).map_err(|_| {
                        InvalidInitArgsSnafu {
                            canister,
                            path: file_path,
                        }
                        .build()
                    })
                }
            }
        }

        // Explicit inline content.
        ManifestInitArgs::Content { content, format } => match format {
            Some(InitArgsFormat::Bin) => BinFormatInlineContentSnafu { canister }.fail(),
            Some(fmt) => Ok(InitArgs::Text {
                content: content.trim().to_owned(),
                format: fmt.clone(),
            }),
            None => Ok(InitArgs::detect(content.clone().into_bytes())
                .expect("inline content string is always valid UTF-8")),
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
                        if !p.join(CANISTER_MANIFEST).is_file() {
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
                    let init_args = m
                        .init_args
                        .as_ref()
                        .map(|mia| resolve_manifest_init_args(mia, &cdir, &m.name))
                        .transpose()?;

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
                            init_args,
                        },
                    ));
                }
            }
        }
    }

    // Networks
    let mut networks: HashMap<String, Network> = HashMap::new();

    // Add IC network first - this is always protected and non-overridable
    networks.insert(
        IC.to_string(),
        Network {
            name: IC.to_string(),
            configuration: Configuration::Connected {
                connected: Connected {
                    api_url: IC_MAINNET_NETWORK_API_URL.parse().unwrap(),
                    http_gateway_url: Some(IC_MAINNET_NETWORK_GATEWAY_URL.parse().unwrap()),
                    // Will use the IC Root key hard coded in agent-rs.
                    // https://github.com/dfinity/agent-rs/blob/b77f1fc5fe05d8de1065ee4cec837bc3f2ce9976/ic-agent/src/agent/mod.rs#L82
                    root_key: None,
                },
            },
        },
    );

    // Track which network names are protected (only IC network)
    let protected_network_names: HashSet<String> = [IC.to_string()].into_iter().collect();

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
    if !networks.contains_key(LOCAL) {
        networks.insert(
            LOCAL.to_string(),
            Network {
                name: LOCAL.to_string(),
                configuration: Configuration::Managed {
                    managed: Managed {
                        mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                            gateway: Gateway {
                                host: DEFAULT_LOCAL_NETWORK_HOST.to_string(),
                                port: Port::Fixed(DEFAULT_LOCAL_NETWORK_PORT),
                            },
                            artificial_delay_ms: None,
                            ii: false,
                            nns: false,
                            subnets: None,
                            version: None,
                        })),
                    },
                },
            },
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
                            for (canister_name, manifest_init_args) in init_args_overrides {
                                if let Some((canister_path, canister)) = cs.get_mut(canister_name) {
                                    canister.init_args = Some(resolve_manifest_init_args(
                                        manifest_init_args,
                                        canister_path,
                                        canister_name,
                                    )?);
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
    // Now we add the implicit `local` and `ic` environment if the user hasn't overriden it
    if let Entry::Vacant(vacant_entry) = environments.entry(LOCAL.to_string()) {
        vacant_entry.insert(Environment {
            name: LOCAL.to_string(),
            network: networks
                .get(LOCAL)
                .ok_or(
                    InvalidNetworkSnafu {
                        environment: LOCAL.to_owned(),
                        network: LOCAL.to_owned(),
                    }
                    .build(),
                )?
                .to_owned(),
            canisters: canisters.clone(),
        });
    }
    if let Entry::Vacant(vacant_entry) = environments.entry(IC.to_string()) {
        vacant_entry.insert(Environment {
            name: IC.to_string(),
            network: networks
                .get(IC)
                .ok_or(
                    InvalidNetworkSnafu {
                        environment: IC.to_owned(),
                        network: IC.to_owned(),
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
