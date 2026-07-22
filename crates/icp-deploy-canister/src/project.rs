use std::collections::{BTreeMap, HashMap, HashSet, hash_map::Entry};

use indexmap::{IndexMap, map::Entry as IndexEntry};

use snafu::prelude::*;

use crate::{
    Canister, Environment, InitArgs, Network, Project,
    canister::{ControllerRef, Settings, recipe},
    fs,
    manifest::{
        ArgsFormat, CANISTER_MANIFEST, CanisterManifest, DependencyManifest, EnvironmentManifest,
        Item, LoadManifestError, ManifestInitArgs, NetworkManifest, PROJECT_MANIFEST,
        ProjectManifest,
        canister::{Instructions, SyncSteps},
        environment::CanisterSelection,
        load_manifest,
        network::RootKeySpec,
        recipe::RecipeType,
    },
    network::{
        Configuration, Connected, DEFAULT_LOCAL_NETWORK_BIND, DEFAULT_LOCAL_NETWORK_PORT, Gateway,
        Managed, ManagedLauncherConfig, ManagedMode, Port,
    },
    prelude::*,
};

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
    #[snafu(display("failed to perform glob parsing"))]
    GlobParse { source: glob::PatternError },

    #[snafu(display("failed to get glob iter"))]
    GlobIter { source: glob::GlobError },

    #[snafu(display("failed to convert path to UTF-8"))]
    Utf8Path { source: FromPathBufError },

    #[snafu(display("failed to load canister manifest"))]
    LoadCanister { source: LoadManifestError },

    #[snafu(display("failed to load network manifest"))]
    LoadNetwork { source: LoadManifestError },

    #[snafu(display("failed to load environment manifest"))]
    LoadEnvironment { source: LoadManifestError },

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
        "init_args for canister '{canister}' uses format 'bin' with inline content; \
         binary format requires a file path"
    ))]
    BinFormatInlineContent { canister: String },

    #[snafu(display(
        "canister '{canister}' lists controller '{controller}', but no canister with that \
         name is declared in the project"
    ))]
    UnknownControllerCanister {
        canister: String,
        controller: String,
    },

    #[snafu(display(
        "canister name '{name}' is invalid: only ASCII letters, digits, '_' and '-' are allowed \
         (':' is reserved as the dependency namespace separator)"
    ))]
    InvalidCanisterName { name: String },

    #[snafu(display(
        "dependency alias '{alias}' is invalid: only ASCII letters, digits, '_' and '-' are allowed \
         (':' is reserved as the dependency namespace separator)"
    ))]
    InvalidDependencyAlias { alias: String },

    #[snafu(display("project declares two dependencies with the same alias '{alias}'"))]
    DuplicateDependencyAlias { alias: String },

    #[snafu(display(
        "dependency alias '{alias}' collides with a canister of the same name in the same project"
    ))]
    DependencyAliasCollision { alias: String },

    #[snafu(display("could not find a project manifest for dependency '{alias}' at: '{path}'"))]
    DependencyNotFound { alias: String, path: String },

    #[snafu(display("failed to canonicalize path for dependency '{alias}' at: '{path}'"))]
    DependencyCanonicalize { alias: String, path: String },

    #[snafu(display("failed to load project manifest for dependency '{alias}'"))]
    LoadDependencyManifest {
        source: LoadManifestError,
        alias: String,
    },

    #[snafu(display(
        "dependency '{alias}' selects canister '{canister}', which the dependency does not declare"
    ))]
    UnknownDependencyCanister { alias: String, canister: String },

    #[snafu(display("dependency cycle detected: {chain}"))]
    CircularDependency { chain: String },

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
        ManifestInitArgs::String(content) => Ok(InitArgs::Text {
            content: content.trim().to_owned(),
            format: ArgsFormat::Candid,
        }),
        ManifestInitArgs::Path { path, format } => {
            let file_path = base_path.join(path);
            match format {
                ArgsFormat::Bin => {
                    let bytes = fs::read(&file_path).context(ReadInitArgsSnafu { canister })?;
                    Ok(InitArgs::Binary(bytes))
                }
                fmt => {
                    let content =
                        fs::read_to_string(&file_path).context(ReadInitArgsSnafu { canister })?;
                    Ok(InitArgs::Text {
                        content: content.trim().to_owned(),
                        format: fmt.clone(),
                    })
                }
            }
        }
        ManifestInitArgs::Value { value, format } => match format {
            ArgsFormat::Bin => BinFormatInlineContentSnafu { canister }.fail(),
            fmt => Ok(InitArgs::Text {
                content: value.trim().to_owned(),
                format: fmt.clone(),
            }),
        },
    }
}

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

/// Whether `name` is a valid canister name or dependency alias: non-empty and
/// containing only ASCII letters, digits, `_`, or `-`.
///
/// A single strict rule keeps names safe for every purpose they are reused for —
/// store-key segments, `PUBLIC_CANISTER_ID:<name>` env vars, DNS subdomains, and
/// archive paths — so no per-site sanitizing is needed. In particular `:` is the
/// dependency namespace separator, and `.` / `/` would be ambiguous in
/// subdomains and paths.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Builds the canonical canisters declared directly in one project manifest,
/// resolving glob/path/inline entries, recipes, and init-args relative to
/// `pdir`. Returns `(local name, canister dir, canister)` with empty bindings;
/// callers assign store keys and bindings. Does not check for duplicate names
/// across projects — that is the caller's responsibility (via the global map).
async fn build_manifest_canisters(
    pdir: &Path,
    manifest_canisters: &[Item<CanisterManifest>],
    recipe_resolver: &dyn recipe::RemoteResourceResolve,
) -> Result<Vec<(String, PathBuf, Canister)>, ConsolidateManifestError> {
    let mut result: Vec<(String, PathBuf, Canister)> = Vec::new();

    for i in manifest_canisters {
        let ms = match i {
            Item::Path(pattern) => {
                let is_glob_pattern = is_glob(pattern);
                let paths = match is_glob_pattern {
                    // Explicit path
                    false => vec![pdir.join(pattern)],

                    // Glob pattern
                    true => {
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
                        p.to_owned(),
                        load_manifest::<CanisterManifest>(&p.join(CANISTER_MANIFEST))
                            .context(LoadCanisterSnafu)?,
                    ));
                }
                ms
            }

            Item::Manifest(m) => vec![(pdir.to_owned(), m.to_owned())],
        };

        for (cdir, m) in ms {
            if !is_valid_name(&m.name) {
                return InvalidCanisterNameSnafu {
                    name: m.name.clone(),
                }
                .fail();
            }

            let registry_recipe = match &m.instructions {
                Instructions::BuildSync { .. } => None,
                Instructions::Recipe { recipe } => match &recipe.recipe_type {
                    RecipeType::Registry { .. } => Some(recipe.recipe_type.to_string()),
                    _ => None,
                },
            };

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
                    let ctx = recipe::RecipeContext {
                        canister_name: m.name.clone(),
                    };
                    recipe_resolver
                        .resolve_recipe(recipe, &ctx)
                        .await
                        .context(RecipeSnafu {
                            recipe_type: recipe.recipe_type.clone(),
                        })?
                }
            };

            let init_args = m
                .init_args
                .as_ref()
                .map(|mia| resolve_manifest_init_args(mia, &cdir, &m.name))
                .transpose()?;

            result.push((
                m.name.clone(),
                cdir,
                Canister {
                    name: m.name.clone(),
                    settings: m.settings.clone(),
                    build,
                    sync,
                    init_args,
                    registry_recipe,
                    bindings: BTreeMap::new(),
                    // Default to the bare local name; overwritten with the
                    // dot-nested alias form when the canister is imported as a
                    // dependency (see `import_dependency`).
                    friendly_names: vec![m.name.clone()],
                },
            ));
        }
    }

    Ok(result)
}

/// A dependency instance imported into the workspace. Returned by
/// [`import_dependency`] and cached per canonical path so diamond dependencies
/// reuse the same instance.
#[derive(Clone)]
struct ImportedInstance {
    /// This instance's own canisters, as `(local name, full store key)` — the
    /// set exposable to the parent via `canisters:` selection.
    own: Vec<(String, String)>,
    /// Every canister in this instance's subtree (its own canisters plus all
    /// transitively imported ones), as `(store key, local name, alias chain
    /// from this instance down to the canister's owning project)`. Used to
    /// register a friendly URL per alias chain when the instance is reached
    /// again via de-duplication (a diamond), including for its descendants.
    subtree: Vec<(String, String, Vec<String>)>,
}

/// A member environment's per-canister config, to be folded into the root's
/// same-named environment beneath any root overrides.
#[derive(Default, Clone)]
struct MemberCanisterOverride {
    settings: Option<Settings>,
    init_args: Option<ManifestInitArgs>,
}

/// Per-environment member overrides: env name → store key → override.
type MemberEnvOverrides = HashMap<String, HashMap<String, MemberCanisterOverride>>;

/// A member's identity (store-key prefix) and the environment names it defines,
/// used to enforce that a member declares every environment the root targets
/// (strict rule).
struct MemberEnvInfo {
    prefix: String,
    defined: HashSet<String>,
}

/// Canonicalize a dependency root (resolving symlinks and `..`) for use as a
/// de-dup / cycle-detection identity.
fn canonicalize_dep(alias: &str, dep_root: &Path) -> Result<PathBuf, ConsolidateManifestError> {
    let build_err = || {
        DependencyCanonicalizeSnafu {
            alias: alias.to_owned(),
            path: dep_root.to_string(),
        }
        .build()
    };
    let canon = dunce::canonicalize(dep_root.as_std_path()).map_err(|_| build_err())?;
    PathBuf::try_from(canon).map_err(|_| build_err())
}

/// Store-key prefix for a dependency instance: its canonical directory relative
/// to the canonical app root, forward-slash separated so keys are stable across
/// platforms and independent of how each edge spells the path.
fn relative_prefix(app_root_canonical: &Path, dep_canonical: &Path) -> String {
    let rel = pathdiff::diff_utf8_paths(dep_canonical, app_root_canonical)
        .unwrap_or_else(|| dep_canonical.to_owned());
    rel.as_str().replace('\\', "/")
}

/// Build a dependency canister's friendly-URL subdomain prefix: the canister's
/// local name as the most-specific label, followed by its alias chain reversed
/// (root-most alias last). E.g. local `backend` reached via `[service-a,
/// openemail]` → `backend.openemail.service-a`. Dot-nested so it stays a valid,
/// collision-free multi-label host; see DESIGN §17.2.
fn friendly_name_for(local: &str, alias_chain: &[String]) -> String {
    let mut labels = Vec::with_capacity(alias_chain.len() + 1);
    labels.push(local.to_string());
    labels.extend(alias_chain.iter().rev().cloned());
    labels.join(".")
}

/// Rewrite `CanisterName` controller references from a dependency's local
/// canister names to their store keys, so global controller validation and
/// deploy-time id lookup operate uniformly on store keys.
fn translate_controllers(canister: &mut Canister, local_to_key: &BTreeMap<String, String>) {
    translate_settings_controllers(&mut canister.settings, local_to_key);
}

/// Rewrite `CanisterName` controller references in a `Settings` from a
/// dependency's local canister names to their store keys.
fn translate_settings_controllers(
    settings: &mut Settings,
    local_to_key: &BTreeMap<String, String>,
) {
    if let Some(controllers) = &mut settings.controllers {
        for cref in controllers.iter_mut() {
            if let ControllerRef::CanisterName(name) = cref
                && let Some(key) = local_to_key.get(name)
            {
                *name = key.clone();
            }
        }
    }
}

/// Compute the `PUBLIC_CANISTER_ID` env-var wiring for canisters in one project
/// scope: its own canisters by local name, plus each dependency's exposed
/// canisters under `<alias>:<canister>`.
fn compute_bindings(
    own: &[(String, String)],
    edges: &[(String, Vec<(String, String)>)],
) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    for (local, key) in own {
        bindings.insert(local.clone(), key.clone());
    }
    for (alias, exposed) in edges {
        for (dep_local, key) in exposed {
            bindings.insert(format!("{alias}:{dep_local}"), key.clone());
        }
    }
    bindings
}

/// Select which of a dependency instance's own canisters are exposed to the
/// parent, per the dependency's `canisters` selection.
fn select_exposed(
    own: &[(String, String)],
    selection: &CanisterSelection,
    alias: &str,
) -> Result<Vec<(String, String)>, ConsolidateManifestError> {
    match selection {
        CanisterSelection::Everything => Ok(own.to_vec()),
        CanisterSelection::None => Ok(vec![]),
        CanisterSelection::Named(names) => {
            let mut out = Vec::new();
            for name in names {
                match own.iter().find(|(local, _)| local == name) {
                    Some(pair) => out.push(pair.clone()),
                    None => {
                        return UnknownDependencyCanisterSnafu {
                            alias: alias.to_owned(),
                            canister: name.clone(),
                        }
                        .fail();
                    }
                }
            }
            Ok(out)
        }
    }
}

/// Validate the dependency aliases declared in one project scope: no `:`, no
/// collision with a local canister name, and no duplicate alias.
fn validate_dependency_aliases(
    deps: &[DependencyManifest],
    own_canister_names: &HashSet<String>,
) -> Result<(), ConsolidateManifestError> {
    let mut seen: HashSet<&str> = HashSet::new();
    for d in deps {
        if !is_valid_name(&d.name) {
            return InvalidDependencyAliasSnafu {
                alias: d.name.clone(),
            }
            .fail();
        }
        if own_canister_names.contains(&d.name) {
            return DependencyAliasCollisionSnafu {
                alias: d.name.clone(),
            }
            .fail();
        }
        if !seen.insert(&d.name) {
            return DuplicateDependencyAliasSnafu {
                alias: d.name.clone(),
            }
            .fail();
        }
    }
    Ok(())
}

/// Recursively import a dependency's canisters into `canisters`, keyed by their
/// app-root-relative store keys. De-duplicates instances by canonical path
/// (diamond dependencies deploy once) and detects cycles. Returns the imported
/// instance's prefix and its own canisters.
#[allow(clippy::too_many_arguments)]
async fn import_dependency(
    app_root_canonical: &Path,
    parent_dir: &Path,
    dep: &DependencyManifest,
    recipe_resolver: &dyn recipe::RemoteResourceResolve,
    canisters: &mut IndexMap<String, (PathBuf, Canister)>,
    registry: &mut HashMap<PathBuf, ImportedInstance>,
    stack: &mut Vec<PathBuf>,
    member_env_overrides: &mut MemberEnvOverrides,
    members: &mut Vec<MemberEnvInfo>,
    // Alias chain from the workspace root to and including this dependency,
    // used to build friendly-URL subdomains (§17.2).
    alias_chain: &[String],
) -> Result<ImportedInstance, ConsolidateManifestError> {
    let dep_root = parent_dir.join(&dep.path);
    let manifest_path = dep_root.join(PROJECT_MANIFEST);
    if !manifest_path.is_file() {
        return DependencyNotFoundSnafu {
            alias: dep.name.clone(),
            path: dep_root.to_string(),
        }
        .fail();
    }

    let canonical = canonicalize_dep(&dep.name, &dep_root)?;

    // Cycle detection.
    if stack.contains(&canonical) {
        let mut chain: Vec<String> = stack.iter().map(|p| p.to_string()).collect();
        chain.push(canonical.to_string());
        return CircularDependencySnafu {
            chain: chain.join(" -> "),
        }
        .fail();
    }

    // Diamond de-dup: same resolved directory means the same instance, deployed
    // once. It is still reachable via this new alias chain, so register an
    // additional friendly URL per chain (§17.3) rather than picking one — for
    // the whole subtree (its own canisters *and* its transitive dependencies),
    // each named by this chain extended with the canister's alias path below the
    // instance.
    if let Some(inst) = registry.get(&canonical) {
        let inst = inst.clone();
        for (key, local, rel_chain) in &inst.subtree {
            let mut chain = alias_chain.to_vec();
            chain.extend(rel_chain.iter().cloned());
            let fname = friendly_name_for(local, &chain);
            if let Some((_, canister)) = canisters.get_mut(key)
                && !canister.friendly_names.contains(&fname)
            {
                canister.friendly_names.push(fname);
            }
        }
        return Ok(inst);
    }

    stack.push(canonical.clone());

    let prefix = relative_prefix(app_root_canonical, &canonical);

    let dep_manifest: ProjectManifest =
        load_manifest(&manifest_path).context(LoadDependencyManifestSnafu {
            alias: dep.name.clone(),
        })?;

    // Build the dependency's own canisters and key them under the prefix. All of
    // them are imported (deploy-all); the `canisters` exposure subset is applied
    // by the caller when wiring env vars.
    let built =
        build_manifest_canisters(&dep_root, &dep_manifest.canisters, recipe_resolver).await?;

    let mut own: Vec<(String, String)> = Vec::new();
    let mut local_to_key: BTreeMap<String, String> = BTreeMap::new();
    for (local, cdir, mut canister) in built {
        let store_key = format!("{prefix}:{local}");
        canister.name = store_key.clone();
        // Friendly URL from the alias chain, not the path-based store key.
        canister.friendly_names = vec![friendly_name_for(&local, alias_chain)];
        own.push((local.clone(), store_key.clone()));
        local_to_key.insert(local.clone(), store_key.clone());
        match canisters.entry(store_key.clone()) {
            IndexEntry::Occupied(_) => {
                return DuplicateSnafu {
                    kind: "canister".to_string(),
                    name: store_key,
                }
                .fail();
            }
            IndexEntry::Vacant(e) => {
                e.insert((cdir, canister));
            }
        }
    }

    // Now that every sibling's store key is known, translate the dependency's
    // controller references (local sibling name -> store key).
    for (_, key) in &own {
        if let Some((_, canister)) = canisters.get_mut(key) {
            translate_controllers(canister, &local_to_key);
        }
    }

    // Capture the member's own environments so the parent can honor its
    // per-canister settings/init_args for the same-named environment
    // (standalone-equivalence). The network binding and canister selection are
    // ignored; only overrides on the member's *own* canisters are
    // folded in — keys naming its dependencies are left to those dependencies.
    let mut defined_envs: HashSet<String> = HashSet::new();
    for env_item in &dep_manifest.environments {
        let em: EnvironmentManifest = match env_item {
            Item::Manifest(m) => m.clone(),
            Item::Path(path) => {
                let p = dep_root.join(path);
                if !p.is_file() {
                    return NotFoundSnafu {
                        kind: "environment".to_string(),
                        path: p.to_string(),
                    }
                    .fail();
                }
                load_manifest::<EnvironmentManifest>(&p).context(LoadEnvironmentSnafu)?
            }
        };
        defined_envs.insert(em.name.clone());
        if let Some(settings) = &em.settings {
            for (local, s) in settings {
                if let Some(key) = local_to_key.get(local) {
                    // Translate the override's own controller references from the
                    // member's local names to store keys, so name-based controllers
                    // resolve against the workspace id map just like base settings.
                    let mut s = s.clone();
                    translate_settings_controllers(&mut s, &local_to_key);
                    member_env_overrides
                        .entry(em.name.clone())
                        .or_default()
                        .entry(key.clone())
                        .or_default()
                        .settings = Some(s);
                }
            }
        }
        if let Some(init_args) = &em.init_args {
            for (local, ia) in init_args {
                if let Some(key) = local_to_key.get(local) {
                    member_env_overrides
                        .entry(em.name.clone())
                        .or_default()
                        .entry(key.clone())
                        .or_default()
                        .init_args = Some(ia.clone());
                }
            }
        }
    }
    members.push(MemberEnvInfo {
        prefix: prefix.clone(),
        defined: defined_envs,
    });

    // Recurse into the dependency's own dependencies.
    let own_names: HashSet<String> = own.iter().map(|(l, _)| l.clone()).collect();
    validate_dependency_aliases(&dep_manifest.dependencies, &own_names)?;

    // The instance's subtree, for diamond-hit friendly-URL propagation: its own
    // canisters sit at the instance root (empty relative alias chain); each
    // nested dependency contributes its subtree prefixed with the nested alias.
    let mut subtree: Vec<(String, String, Vec<String>)> = own
        .iter()
        .map(|(local, key)| (key.clone(), local.clone(), Vec::new()))
        .collect();

    let mut edges: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for nested in &dep_manifest.dependencies {
        let mut nested_chain = alias_chain.to_vec();
        nested_chain.push(nested.name.clone());
        let inst = Box::pin(import_dependency(
            app_root_canonical,
            &dep_root,
            nested,
            recipe_resolver,
            canisters,
            registry,
            stack,
            member_env_overrides,
            members,
            &nested_chain,
        ))
        .await?;
        for (key, local, rel) in &inst.subtree {
            let mut r = Vec::with_capacity(rel.len() + 1);
            r.push(nested.name.clone());
            r.extend(rel.iter().cloned());
            subtree.push((key.clone(), local.clone(), r));
        }
        let exposed = select_exposed(&inst.own, &nested.canisters, &nested.name)?;
        edges.push((nested.name.clone(), exposed));
    }

    // Assign env-var bindings for this instance's own canisters.
    let bindings = compute_bindings(&own, &edges);
    for (_, key) in &own {
        if let Some((_, canister)) = canisters.get_mut(key) {
            canister.bindings = bindings.clone();
        }
    }

    stack.pop();
    let instance = ImportedInstance { own, subtree };
    registry.insert(canonical, instance.clone());
    Ok(instance)
}

/// Build one environment's canister map: select from `canisters`, then apply the
/// member overrides for this environment (standalone-equivalence), then
/// the root's own overrides (highest precedence). Precedence is therefore
/// root-explicit > member-env > canister-base.
fn build_environment_canisters(
    canisters: &IndexMap<String, (PathBuf, Canister)>,
    env_name: &str,
    selection: &CanisterSelection,
    member_overrides: Option<&HashMap<String, MemberCanisterOverride>>,
    root_settings: Option<&HashMap<String, Settings>>,
    root_init_args: Option<&HashMap<String, ManifestInitArgs>>,
) -> Result<IndexMap<String, (PathBuf, Canister)>, ConsolidateManifestError> {
    let mut cs = match selection {
        CanisterSelection::None => IndexMap::new(),
        CanisterSelection::Everything => canisters.clone(),
        CanisterSelection::Named(names) => {
            let mut cs: IndexMap<String, (PathBuf, Canister)> = IndexMap::new();
            for name in names {
                let v = canisters.get(name).ok_or(
                    InvalidCanisterSnafu {
                        environment: env_name.to_owned(),
                        canister: name.to_owned(),
                    }
                    .build(),
                )?;
                cs.insert(name.to_owned(), v.to_owned());
            }
            cs
        }
    };

    // Member overrides first (lower precedence than the root's own overrides).
    if let Some(overrides) = member_overrides {
        for (key, ov) in overrides {
            if let Some((cpath, canister)) = cs.get_mut(key) {
                if let Some(s) = &ov.settings {
                    canister.settings = s.clone();
                }
                if let Some(ia) = &ov.init_args {
                    canister.init_args = Some(resolve_manifest_init_args(ia, cpath, key)?);
                }
            }
        }
    }

    // Root overrides last (highest precedence).
    if let Some(settings) = root_settings {
        for (name, s) in settings {
            if let Some((_p, canister)) = cs.get_mut(name) {
                canister.settings = s.clone();
            }
        }
    }
    if let Some(init_args) = root_init_args {
        for (name, ia) in init_args {
            if let Some((cpath, canister)) = cs.get_mut(name) {
                canister.init_args = Some(resolve_manifest_init_args(ia, cpath, name)?);
            }
        }
    }

    Ok(cs)
}

/// Turns the ProjectManifest into a Project struct
/// - Adds the default Networks
/// - Adds the default Environment
/// - Imports any dependency projects' canisters
/// - Validates the manifest to make sure that:
///     - There are no duplicates
///     - All the environments have networks
///     - All the referenced canisters exist
///     - All the recipes have been resolved
pub async fn consolidate_manifest(
    pdir: &Path,
    recipe_resolver: &dyn recipe::RemoteResourceResolve,
    m: &ProjectManifest,
) -> Result<Project, ConsolidateManifestError> {
    // Canisters. IndexMap (not HashMap) so the order from the project manifest is preserved
    // through to consumers like `icp project bundle`, which needs reproducible output.
    let mut canisters: IndexMap<String, (PathBuf, Canister)> = IndexMap::new();

    // Canonical app root, used to derive stable, order-independent store-key
    // prefixes for imported dependency canisters.
    let app_root_canonical =
        canonicalize_dep("<project>", pdir).unwrap_or_else(|_| pdir.to_owned());

    // This project's own canisters, keyed by their bare local names.
    let app_built = build_manifest_canisters(pdir, &m.canisters, recipe_resolver).await?;
    let mut app_own: Vec<(String, String)> = Vec::new();
    for (local, cdir, canister) in app_built {
        app_own.push((local.clone(), local.clone()));
        match canisters.entry(local.clone()) {
            IndexEntry::Occupied(e) => {
                return DuplicateSnafu {
                    kind: "canister".to_string(),
                    name: e.key().to_owned(),
                }
                .fail();
            }
            IndexEntry::Vacant(e) => {
                e.insert((cdir, canister));
            }
        }
    }

    // Import dependency projects. Each dependency is deployed in full and keyed
    // under its app-root-relative path; diamonds (the same directory reached via
    // multiple edges) resolve to a single instance.
    let mut registry: HashMap<PathBuf, ImportedInstance> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    // Member environment config folded into the root's same-named environments,
    // and the per-member set of declared environment names for the strict rule.
    let mut member_env_overrides: MemberEnvOverrides = HashMap::new();
    let mut members: Vec<MemberEnvInfo> = Vec::new();
    let app_own_names: HashSet<String> = app_own.iter().map(|(l, _)| l.clone()).collect();
    validate_dependency_aliases(&m.dependencies, &app_own_names)?;

    let mut app_edges: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for dep in &m.dependencies {
        let inst = import_dependency(
            &app_root_canonical,
            pdir,
            dep,
            recipe_resolver,
            &mut canisters,
            &mut registry,
            &mut stack,
            &mut member_env_overrides,
            &mut members,
            std::slice::from_ref(&dep.name),
        )
        .await?;
        let exposed = select_exposed(&inst.own, &dep.canisters, &dep.name)?;
        app_edges.push((dep.name.clone(), exposed));
    }

    // Assign env-var bindings for this project's own canisters (own canisters by
    // local name plus each dependency's exposed canisters under `<alias>:<name>`).
    let app_bindings = compute_bindings(&app_own, &app_edges);
    for (_, key) in &app_own {
        if let Some((_, canister)) = canisters.get_mut(key) {
            canister.bindings = app_bindings.clone();
        }
    }

    // Friendly URLs need no de-collision pass: the strict name rule (no '.') makes
    // own canisters single-label and dependency canisters multi-label (dot-nested
    // by alias chain), so their hostnames are disjoint by construction (§17.2).

    // Validate that every canister-name controller reference points to a declared canister.
    // Catching typos here turns "perpetual warning" into a clear load-time error.
    for (canister_name, (_, canister)) in &canisters {
        let Some(crefs) = &canister.settings.controllers else {
            continue;
        };
        for cref in crefs {
            if let Some(ref_name) = cref.canister_name()
                && !canisters.contains_key(ref_name)
            {
                return UnknownControllerCanisterSnafu {
                    canister: canister_name.to_owned(),
                    controller: ref_name.to_owned(),
                }
                .fail();
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
                    root_key: RootKeySpec::Mainnet,
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
                load_manifest::<NetworkManifest>(&path).context(LoadNetworkSnafu)?
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
                                bind: DEFAULT_LOCAL_NETWORK_BIND.to_string(),
                                port: Port::Fixed(DEFAULT_LOCAL_NETWORK_PORT),
                                domains: vec![],
                            },
                            artificial_delay_ms: None,
                            ii: false,
                            nns: false,
                            subnets: None,
                            bitcoind_addr: None,
                            dogecoind_addr: None,
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
                load_manifest::<EnvironmentManifest>(&path).context(LoadEnvironmentSnafu)?
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

                    // Embed canisters in environment, folding member overrides
                    // beneath the root's own settings/init_args overrides.
                    canisters: build_environment_canisters(
                        &canisters,
                        &m.name,
                        &m.canisters,
                        member_env_overrides.get(&m.name),
                        m.settings.as_ref(),
                        m.init_args.as_ref(),
                    )?,
                });
            }
        }
    }

    // We're done adding all the user environments
    // Now we add the implicit `local` and `ic` environment if the user hasn't overriden it
    if let Entry::Vacant(vacant_entry) = environments.entry(LOCAL.to_string()) {
        let network = networks
            .get(LOCAL)
            .ok_or(
                InvalidNetworkSnafu {
                    environment: LOCAL.to_owned(),
                    network: LOCAL.to_owned(),
                }
                .build(),
            )?
            .to_owned();
        vacant_entry.insert(Environment {
            name: LOCAL.to_string(),
            network,
            canisters: build_environment_canisters(
                &canisters,
                LOCAL,
                &CanisterSelection::Everything,
                member_env_overrides.get(LOCAL),
                None,
                None,
            )?,
        });
    }
    if let Entry::Vacant(vacant_entry) = environments.entry(IC.to_string()) {
        let network = networks
            .get(IC)
            .ok_or(
                InvalidNetworkSnafu {
                    environment: IC.to_owned(),
                    network: IC.to_owned(),
                }
                .build(),
            )?
            .to_owned();
        vacant_entry.insert(Environment {
            name: IC.to_string(),
            network,
            canisters: build_environment_canisters(
                &canisters,
                IC,
                &CanisterSelection::Everything,
                member_env_overrides.get(IC),
                None,
                None,
            )?,
        });
    }

    // Strict rule: every member must declare each environment the root targets.
    // `local`/`ic` are implicit for every project, so they never count
    // as missing; other environments must be declared explicitly by the member.
    // Recorded per-environment and enforced lazily when that environment is
    // selected (so a missing `staging` never blocks `deploy -e local`).
    let mut member_missing_envs: HashMap<String, Vec<String>> = HashMap::new();
    for env_name in environments.keys() {
        if env_name == LOCAL || env_name == IC {
            continue;
        }
        for member in &members {
            if !member.defined.contains(env_name) {
                member_missing_envs
                    .entry(env_name.clone())
                    .or_default()
                    .push(member.prefix.clone());
            }
        }
    }

    Ok(Project {
        dir: pdir.into(),
        canisters,
        networks,
        environments,
        member_missing_envs,
    })
}

#[derive(Debug, Snafu)]
pub enum LoadProjectError {
    #[snafu(display("failed to load project manifest"))]
    ProjectManifest { source: LoadManifestError },

    #[snafu(transparent)]
    Consolidate { source: ConsolidateManifestError },
}

/// Load and consolidate the project rooted at `project_dir` (already located by
/// the caller), resolving recipes through `recipe`.
pub async fn load_project(
    recipe: &dyn recipe::RemoteResourceResolve,
    project_dir: &Path,
) -> Result<Project, LoadProjectError> {
    let m: ProjectManifest =
        load_manifest(&project_dir.join(PROJECT_MANIFEST)).context(ProjectManifestSnafu)?;
    let p = consolidate_manifest(project_dir, recipe, &m).await?;
    Ok(p)
}

#[derive(Debug, Snafu)]
pub enum VerifySandboxError {
    #[snafu(display(
        "canister '{canister}' uses a script {phase} step, which cannot run in the sandbox; \
         only pre-built builds and plugin syncs are permitted"
    ))]
    ScriptStep { canister: String, phase: String },
}

/// Verify that a fully-resolved project (recipes already resolved into concrete
/// steps) contains no script steps. Script build/sync steps spawn host
/// subprocesses and therefore cannot run inside the sandbox; only pre-built
/// builds and plugin syncs are permitted.
pub fn verify_sandbox(project: &Project) -> Result<(), VerifySandboxError> {
    use crate::manifest::canister::{BuildStep, SyncStep};

    for (name, (_, canister)) in &project.canisters {
        if canister
            .build
            .steps
            .iter()
            .any(|s| matches!(s, BuildStep::Script(_)))
        {
            return ScriptStepSnafu {
                canister: name.clone(),
                phase: "build",
            }
            .fail();
        }
        if canister
            .sync
            .steps
            .iter()
            .any(|s| matches!(s, SyncStep::Script(_)))
        {
            return ScriptStepSnafu {
                canister: name.clone(),
                phase: "sync",
            }
            .fail();
        }
    }
    Ok(())
}

#[cfg(test)]
mod dependency_tests {
    use super::*;
    use crate::canister::recipe::{RecipeContext, RemoteResourceResolve, ResolveError};
    use crate::manifest::adapter::prebuilt::SourceField;
    use crate::manifest::canister::{BuildSteps, SyncSteps};
    use crate::manifest::recipe::Recipe;
    use camino_tempfile::Utf8TempDir;
    use tokio::sync::mpsc::Sender;

    /// Recipes and plugins are never used in these tests; every canister is pre-built.
    struct PanicResolver;

    #[async_trait::async_trait]
    impl RemoteResourceResolve for PanicResolver {
        async fn resolve_recipe(
            &self,
            _recipe: &Recipe,
            _context: &RecipeContext,
        ) -> Result<(BuildSteps, SyncSteps), ResolveError> {
            panic!("recipe resolver should not be called in dependency tests");
        }

        async fn resolve_wasm(
            &self,
            _source: &SourceField,
            _base_dir: &Path,
            _sha256: Option<&str>,
            _stdio: Option<Sender<String>>,
        ) -> Result<PathBuf, ResolveError> {
            panic!("wasm resolver should not be called in dependency tests");
        }
    }

    fn write(dir: &Path, rel: &str, contents: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }

    /// A minimal `icp.yaml` body declaring the given pre-built canisters,
    /// followed by a raw `dependencies:` block (may be empty).
    fn manifest(canisters: &[&str], deps: &str) -> String {
        let mut s = String::new();
        if canisters.is_empty() {
            s.push_str("canisters: []\n");
        } else {
            s.push_str("canisters:\n");
            for c in canisters {
                s.push_str(&format!(
                    "  - name: {c}\n    build:\n      steps:\n        - type: pre-built\n          path: {c}.wasm\n"
                ));
            }
        }
        s.push_str(deps);
        s
    }

    async fn consolidate(pdir: &Path) -> Result<Project, ConsolidateManifestError> {
        let m: ProjectManifest =
            load_manifest(&pdir.join(PROJECT_MANIFEST)).expect("failed to parse project manifest");
        consolidate_manifest(pdir, &PanicResolver, &m).await
    }

    fn bindings_of<'a>(p: &'a Project, key: &str) -> &'a BTreeMap<String, String> {
        &p.canisters
            .get(key)
            .unwrap_or_else(|| {
                panic!(
                    "canister '{key}' not found; have {:?}",
                    p.canisters.keys().collect::<Vec<_>>()
                )
            })
            .1
            .bindings
    }

    fn friendly_names_of<'a>(p: &'a Project, key: &str) -> &'a [String] {
        &p.canisters
            .get(key)
            .unwrap_or_else(|| {
                panic!(
                    "canister '{key}' not found; have {:?}",
                    p.canisters.keys().collect::<Vec<_>>()
                )
            })
            .1
            .friendly_names
    }

    #[tokio::test]
    async fn single_project_bindings_are_self_and_siblings() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(&["backend", "frontend"], ""),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // Flat behavior preserved: every canister maps every sibling (incl. self)
        // to itself.
        let expected = BTreeMap::from([
            ("backend".to_string(), "backend".to_string()),
            ("frontend".to_string(), "frontend".to_string()),
        ]);
        assert_eq!(bindings_of(&p, "backend"), &expected);
        assert_eq!(bindings_of(&p, "frontend"), &expected);
    }

    #[tokio::test]
    async fn dependency_import_and_exposure_subset() {
        let tmp = Utf8TempDir::new().unwrap();
        // Dependency nested inside the app (mirrors a submodule under the app).
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend", "frontend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n    canisters: [backend]\n",
            ),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // The whole dependency is deployed (both canisters imported), keyed by path.
        assert!(p.canisters.contains_key("backend"));
        assert!(p.canisters.contains_key("openemail:backend"));
        assert!(p.canisters.contains_key("openemail:frontend"));

        // App's own canister sees itself and only the *exposed* dependency canister.
        assert_eq!(
            bindings_of(&p, "backend"),
            &BTreeMap::from([
                ("backend".to_string(), "backend".to_string()),
                (
                    "openemail:backend".to_string(),
                    "openemail:backend".to_string()
                ),
            ])
        );

        // The dependency's own canisters keep their standalone view (bare names).
        assert_eq!(
            bindings_of(&p, "openemail:backend"),
            &BTreeMap::from([
                ("backend".to_string(), "openemail:backend".to_string()),
                ("frontend".to_string(), "openemail:frontend".to_string()),
            ])
        );
    }

    #[tokio::test]
    async fn member_env_config_folds_in_with_root_override_winning() {
        let tmp = Utf8TempDir::new().unwrap();
        // openemail defines `staging` with per-canister settings for its own
        // canisters.
        write(
            tmp.path(),
            "openemail/icp.yaml",
            r#"
canisters:
  - name: backend
    build:
      steps:
        - type: pre-built
          path: backend.wasm
  - name: frontend
    build:
      steps:
        - type: pre-built
          path: frontend.wasm
environments:
  - name: staging
    settings:
      backend:
        compute_allocation: 5
      frontend:
        compute_allocation: 7
"#,
        );
        // The app declares openemail and also defines `staging`, overriding the
        // imported backend's settings (the root override must win).
        write(
            tmp.path(),
            "icp.yaml",
            r#"
canisters:
  - name: app
    build:
      steps:
        - type: pre-built
          path: app.wasm
dependencies:
  - name: openemail
    path: ./openemail
environments:
  - name: staging
    settings:
      "openemail:backend":
        compute_allocation: 99
"#,
        );

        let p = consolidate(tmp.path()).await.unwrap();
        let staging = p.environments.get("staging").expect("staging environment");

        // Root override wins over the member's config.
        assert_eq!(
            staging
                .canisters
                .get("openemail:backend")
                .unwrap()
                .1
                .settings
                .compute_allocation,
            Some(99),
        );
        // No root override → the member's own config applies (standalone-equivalence).
        assert_eq!(
            staging
                .canisters
                .get("openemail:frontend")
                .unwrap()
                .1
                .settings
                .compute_allocation,
            Some(7),
        );
        // Both projects declared staging, so nothing is recorded as missing.
        assert!(p.member_missing_envs.is_empty());
    }

    #[tokio::test]
    async fn missing_member_environment_is_recorded() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            r#"
canisters:
  - name: app
    build:
      steps:
        - type: pre-built
          path: app.wasm
dependencies:
  - name: openemail
    path: ./openemail
environments:
  - name: staging
"#,
        );

        let p = consolidate(tmp.path()).await.unwrap();
        // openemail does not declare `staging`, so it is recorded as missing.
        assert_eq!(
            p.member_missing_envs.get("staging"),
            Some(&vec!["openemail".to_string()]),
        );
        // Implicit environments are never recorded as missing.
        assert!(!p.member_missing_envs.contains_key("local"));
        assert!(!p.member_missing_envs.contains_key("ic"));
    }

    #[tokio::test]
    async fn diamond_dedups_to_single_instance() {
        let tmp = Utf8TempDir::new().unwrap();
        // umbrella layout: service-a and service-b both depend on ../openemail.
        write(
            tmp.path(),
            "umbrella/openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "umbrella/service-a/icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ../openemail\n",
            ),
        );
        write(
            tmp.path(),
            "umbrella/service-b/icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ../openemail\n",
            ),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &[],
                "dependencies:\n  - name: service-a\n    path: ./umbrella/service-a\n  - name: service-b\n    path: ./umbrella/service-b\n",
            ),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // openemail is imported exactly once despite two edges reaching it.
        let openemail_keys: Vec<_> = p
            .canisters
            .keys()
            .filter(|k| k.contains("openemail"))
            .collect();
        assert_eq!(
            openemail_keys,
            vec![&"umbrella/openemail:backend".to_string()],
            "expected a single shared openemail instance"
        );

        // Both services' code reads `openemail:backend`, resolving to the one instance.
        assert_eq!(
            bindings_of(&p, "umbrella/service-a:backend").get("openemail:backend"),
            Some(&"umbrella/openemail:backend".to_string())
        );
        assert_eq!(
            bindings_of(&p, "umbrella/service-b:backend").get("openemail:backend"),
            Some(&"umbrella/openemail:backend".to_string())
        );

        // The single shared instance is reachable at one friendly URL per alias
        // chain (§17.3) — the store-key path (`umbrella/`) never appears.
        assert_eq!(
            friendly_names_of(&p, "umbrella/openemail:backend"),
            &["backend.openemail.service-a", "backend.openemail.service-b"]
        );
        // Each service's own canister is named by its own alias chain.
        assert_eq!(
            friendly_names_of(&p, "umbrella/service-a:backend"),
            &["backend.service-a"]
        );
        assert_eq!(
            friendly_names_of(&p, "umbrella/service-b:backend"),
            &["backend.service-b"]
        );
    }

    #[tokio::test]
    async fn diamond_transitive_dependency_gets_url_per_chain() {
        let tmp = Utf8TempDir::new().unwrap();
        // The shared openemail itself depends on libfoo, and is reached via both
        // service-a and service-b.
        write(
            tmp.path(),
            "umbrella/openemail/libfoo/icp.yaml",
            &manifest(&["bar"], ""),
        );
        write(
            tmp.path(),
            "umbrella/openemail/icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: libfoo\n    path: ./libfoo\n",
            ),
        );
        write(
            tmp.path(),
            "umbrella/service-a/icp.yaml",
            &manifest(
                &["service"],
                "dependencies:\n  - name: openemail\n    path: ../openemail\n",
            ),
        );
        write(
            tmp.path(),
            "umbrella/service-b/icp.yaml",
            &manifest(
                &["service"],
                "dependencies:\n  - name: openemail\n    path: ../openemail\n",
            ),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &[],
                "dependencies:\n  - name: service-a\n    path: ./umbrella/service-a\n  - name: service-b\n    path: ./umbrella/service-b\n",
            ),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // The shared instance's own canister gets one URL per chain...
        assert_eq!(
            friendly_names_of(&p, "umbrella/openemail:backend"),
            &["backend.openemail.service-a", "backend.openemail.service-b"]
        );
        // ...and so does its *transitive* dependency (the subtree is revisited on
        // the diamond hit, not just the instance's own canisters).
        assert_eq!(
            friendly_names_of(&p, "umbrella/openemail/libfoo:bar"),
            &[
                "bar.libfoo.openemail.service-a",
                "bar.libfoo.openemail.service-b"
            ]
        );
    }

    #[tokio::test]
    async fn dot_in_canister_name_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        // '.' is banned: it would be ambiguous in a dot-nested friendly subdomain
        // (an own canister named `frontend.openemail` could collide with dependency
        // `openemail`'s `frontend`). The strict name rule rejects it up front.
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(&["frontend.openemail"], ""),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(err, ConsolidateManifestError::InvalidCanisterName { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn invalid_dependency_alias_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["app"],
                "dependencies:\n  - name: open.email\n    path: ./openemail\n",
            ),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(err, ConsolidateManifestError::InvalidDependencyAlias { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn member_override_controllers_are_translated_to_store_keys() {
        let tmp = Utf8TempDir::new().unwrap();
        // openemail's `staging` override names a controller by its local name.
        write(
            tmp.path(),
            "openemail/icp.yaml",
            r#"
canisters:
  - name: backend
    build:
      steps:
        - type: pre-built
          path: backend.wasm
  - name: frontend
    build:
      steps:
        - type: pre-built
          path: frontend.wasm
environments:
  - name: staging
    settings:
      backend:
        controllers: ["frontend"]
"#,
        );
        write(
            tmp.path(),
            "icp.yaml",
            r#"
canisters:
  - name: app
    build:
      steps:
        - type: pre-built
          path: app.wasm
dependencies:
  - name: openemail
    path: ./openemail
environments:
  - name: staging
"#,
        );

        let p = consolidate(tmp.path()).await.unwrap();
        let staging = p.environments.get("staging").expect("staging environment");
        let controllers = staging
            .canisters
            .get("openemail:backend")
            .unwrap()
            .1
            .settings
            .controllers
            .clone()
            .expect("controllers set by the member override");

        // The member-local `frontend` must be translated to its store key, so it
        // resolves against the workspace id map at deploy time.
        assert_eq!(
            controllers,
            vec![ControllerRef::CanisterName(
                "openemail:frontend".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn friendly_names_are_bare_for_own_and_dotted_for_dependencies() {
        let tmp = Utf8TempDir::new().unwrap();
        // openemail (with a transitive dep libfoo) vendored under the app.
        write(
            tmp.path(),
            "openemail/libfoo/icp.yaml",
            &manifest(&["bar"], ""),
        );
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(
                &["backend", "frontend"],
                "dependencies:\n  - name: libfoo\n    path: ./libfoo\n",
            ),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n",
            ),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // Own canister: bare name (unchanged from single-project behavior).
        assert_eq!(friendly_names_of(&p, "backend"), &["backend"]);
        // Direct dependency: dot-nested by alias (no `vendor/` path noise).
        assert_eq!(
            friendly_names_of(&p, "openemail:backend"),
            &["backend.openemail"]
        );
        assert_eq!(
            friendly_names_of(&p, "openemail:frontend"),
            &["frontend.openemail"]
        );
        // Transitive dependency: full alias chain, canister-most-specific first.
        assert_eq!(
            friendly_names_of(&p, "openemail/libfoo:bar"),
            &["bar.libfoo.openemail"]
        );
    }

    #[tokio::test]
    async fn cycle_is_detected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(&[], "dependencies:\n  - name: a\n    path: ./a\n"),
        );
        write(
            tmp.path(),
            "a/icp.yaml",
            &manifest(&["x"], "dependencies:\n  - name: b\n    path: ../b\n"),
        );
        write(
            tmp.path(),
            "b/icp.yaml",
            &manifest(&["y"], "dependencies:\n  - name: a\n    path: ../a\n"),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(err, ConsolidateManifestError::CircularDependency { .. }),
            "expected CircularDependency, got {err:?}"
        );
    }

    #[tokio::test]
    async fn alias_colliding_with_canister_name_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["openemail"],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n",
            ),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(
                err,
                ConsolidateManifestError::DependencyAliasCollision { .. }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn duplicate_alias_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(tmp.path(), "one/icp.yaml", &manifest(&["backend"], ""));
        write(tmp.path(), "two/icp.yaml", &manifest(&["backend"], ""));
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &[],
                "dependencies:\n  - name: dup\n    path: ./one\n  - name: dup\n    path: ./two\n",
            ),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(
                err,
                ConsolidateManifestError::DuplicateDependencyAlias { .. }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn colon_in_canister_name_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(tmp.path(), "icp.yaml", &manifest(&["foo:bar"], ""));

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(err, ConsolidateManifestError::InvalidCanisterName { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn unknown_exposed_canister_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &[],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n    canisters: [nope]\n",
            ),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(
                err,
                ConsolidateManifestError::UnknownDependencyCanister { .. }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn missing_dependency_path_is_rejected() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &[],
                "dependencies:\n  - name: openemail\n    path: ./does-not-exist\n",
            ),
        );

        let err = consolidate(tmp.path()).await.unwrap_err();
        assert!(
            matches!(err, ConsolidateManifestError::DependencyNotFound { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn imported_canisters_appear_in_implicit_environments() {
        let tmp = Utf8TempDir::new().unwrap();
        write(
            tmp.path(),
            "openemail/icp.yaml",
            &manifest(&["backend"], ""),
        );
        write(
            tmp.path(),
            "icp.yaml",
            &manifest(
                &["backend"],
                "dependencies:\n  - name: openemail\n    path: ./openemail\n",
            ),
        );

        let p = consolidate(tmp.path()).await.unwrap();

        // Deploy-all: the implicit `local` environment includes the dependency.
        let local = p.environments.get("local").unwrap();
        assert!(local.canisters.contains_key("backend"));
        assert!(local.canisters.contains_key("openemail:backend"));
    }
}
