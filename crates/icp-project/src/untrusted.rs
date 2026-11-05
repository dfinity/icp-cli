//! Holding a project someone else handed over to what it may do.
//!
//! The CLI trusts a project the way it trusts the user's own checkout: a
//! build step may run any command, a recipe may come from any file or URL, a
//! module may be downloaded from anywhere, and a manifest may name any path on
//! the machine. A project from elsewhere — an archive an agent passed to a
//! deploy service, a checkout the user has not read — gets none of that. What
//! it may do is the shape `icp project bundle` writes, since a bundle is what
//! such a project usually is, and it is said as what is allowed rather than
//! what is not, so that a kind of step added later is refused until someone
//! decides otherwise:
//!
//! - a build step is a pre-built module read from a file in the project
//!   ([`build_step_allowed`]);
//! - a sync step is a plugin read from a file in the project
//!   ([`sync_step_allowed`]);
//! - a recipe is never fetched: it is build instructions read from a file or
//!   a URL at load, and a bundle has expanded every one of them;
//! - a path names something under the project root, through no symlink.
//!
//! The last is the business of the [`FileSystem`](crate::files::FileSystem)
//! the project is loaded through — a [`Confined`](crate::files::Confined)
//! that refuses links — because the loader reads argument and variable files
//! into the model as it loads, before any caller could look. The recipe rule
//! is the business of the [`Resolve`] seam for the same reason: the loader
//! asks it for the template, and [`NoRecipes`] says no. The steps can be seen
//! in the loaded project, and [`vet`] holds them to the list there.
//! [`Vetted`] wraps a loader so that every load is vetted before the project
//! is handed out, and a consumer composing its own host holds the three
//! together:
//!
//! ```ignore
//! let files = Arc::new(Confined::refusing_links(&root));
//! let project = Vetted::new(ProjectLoadImpl {
//!     project_root_locate,
//!     recipe: Arc::new(NoRecipes),
//!     files: files.clone(),
//! });
//! ```
//!
//! A checksum on a module is not asked for. It is the bundler's promise about
//! its own archive, and the same untrusted hand wrote both, so it says nothing
//! about what the project can reach.

use async_trait::async_trait;
use snafu::{ResultExt, Snafu};

use crate::{
    Project, ProjectLoad, ProjectLoadError, UntrustedSnafu,
    canister::recipe::{Fetched, Resolve, ResolveError},
    manifest::{
        adapter::prebuilt::SourceField,
        canister::{BuildStep, SyncStep},
        recipe::Recipe,
    },
    prelude::*,
};

/// Why a loaded project may not be acted on as untrusted.
#[derive(Debug, Snafu)]
pub enum Refused {
    #[snafu(display(
        "canister '{canister}' has a build step that is not a pre-built module from a file in \
         the project, which is all an untrusted project may have: {step}"
    ))]
    BuildStep { canister: String, step: String },

    #[snafu(display(
        "canister '{canister}' has a sync step that is not a plugin from a file in the project, \
         which is all an untrusted project may have: {step}"
    ))]
    SyncStep { canister: String, step: String },
}

/// Whether `step` is the one build step an untrusted project may have: a
/// pre-built module read from a file in the project. A script is a host
/// subprocess, and a module fetched from a URL reaches the network and the
/// package cache on the strength of an untrusted manifest.
pub fn build_step_allowed(step: &BuildStep) -> bool {
    matches!(step, BuildStep::Prebuilt(adapter) if matches!(adapter.source, SourceField::Local(_)))
}

/// Whether `step` is the one sync step an untrusted project may have: a
/// plugin read from a file in the project. It runs sandboxed either way; it
/// is the download that is refused.
pub fn sync_step_allowed(step: &SyncStep) -> bool {
    matches!(step, SyncStep::Plugin(adapter) if matches!(adapter.source, SourceField::Local(_)))
}

/// Hold a loaded project's steps to [`build_step_allowed`] and
/// [`sync_step_allowed`], naming the first that is neither.
///
/// What the loader has already read or fetched — argument files, recipe
/// templates — is not here to look at, and is held by the seams it was read
/// through instead.
pub fn vet(project: &Project) -> Result<(), Refused> {
    for (_, (_, canister)) in &project.canisters {
        if let Some(step) = canister
            .build
            .steps
            .iter()
            .find(|step| !build_step_allowed(step))
        {
            return BuildStepSnafu {
                canister: &canister.name,
                step: one_line(step),
            }
            .fail();
        }
        if let Some(step) = canister
            .sync
            .steps
            .iter()
            .find(|step| !sync_step_allowed(step))
        {
            return SyncStepSnafu {
                canister: &canister.name,
                step: one_line(step),
            }
            .fail();
        }
    }
    Ok(())
}

/// A step as it displays, on one line: a step displays its kind and then
/// its fields or commands one per line, and an error message has room for
/// one, so the lines after the first are run together with `;` between.
fn one_line(step: &impl std::fmt::Display) -> String {
    let text = step.to_string();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let kind = lines.next().unwrap_or_default();
    let rest = lines.collect::<Vec<_>>().join("; ");
    if rest.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind} {rest}")
    }
}

/// The [`Resolve`] that fetches nothing.
///
/// A recipe is resolved while the project loads, so a project that names one
/// fails to load rather than loading and then being refused — the one place
/// the rule can be applied before the template is read from wherever the
/// manifest points.
pub struct NoRecipes;

/// A recipe named by a project loaded as untrusted.
#[derive(Debug, Snafu)]
#[snafu(display(
    "recipe '{recipe}' is not fetched: an untrusted project must spell out its build and sync \
     steps, as a bundle does"
))]
pub struct RecipeRefused {
    pub recipe: String,
}

#[async_trait]
impl Resolve for NoRecipes {
    async fn resolve(&self, recipe: &Recipe) -> Result<Fetched, ResolveError> {
        Err(ResolveError::new(RecipeRefused {
            recipe: recipe.recipe_type.to_string(),
        }))
    }
}

/// A [`ProjectLoad`] that hands out only a project [`vet`] accepts.
pub struct Vetted<L>(L);

impl<L> Vetted<L> {
    pub fn new(loader: L) -> Self {
        Self(loader)
    }
}

#[async_trait]
impl<L: ProjectLoad> ProjectLoad for Vetted<L> {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        let project = self.0.load().await?;
        vet(&project).context(UntrustedSnafu)?;
        Ok(project)
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        self.0.exists().await
    }

    fn member_dir(&self) -> Option<PathBuf> {
        self.0.member_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MockProjectLoader,
        manifest::{
            adapter::{prebuilt, script},
            canister::{BuildSteps, SyncSteps},
        },
    };

    async fn project_with(build: Vec<BuildStep>, sync: Vec<SyncStep>) -> Project {
        let mut project = MockProjectLoader::minimal().load().await.unwrap();
        let (_, canister) = project.canisters.get_mut("backend").unwrap();
        canister.build = BuildSteps { steps: build };
        canister.sync = SyncSteps { steps: sync };
        project
    }

    fn local(path: &str) -> BuildStep {
        BuildStep::Prebuilt(prebuilt::Adapter {
            source: SourceField::Local(prebuilt::LocalSource { path: path.into() }),
            sha256: None,
        })
    }

    fn script(command: &str) -> script::Adapter {
        script::Adapter {
            command: script::CommandField::Command(command.to_owned()),
        }
    }

    fn plugin(source: &str) -> SyncStep {
        SyncStep::Plugin(Box::new(
            serde_yaml::from_str(&format!("{source}\nsha256: abc")).unwrap(),
        ))
    }

    #[tokio::test]
    async fn a_bundle_shaped_project_is_accepted() {
        let project = project_with(vec![local("backend.wasm")], vec![plugin("path: p.wasm")]).await;
        vet(&project).expect("pre-built from the project, plugin from the project");
    }

    /// A script step and a remote module are the two other build steps there
    /// are. Neither is on the list, and the message says what the list is
    /// and what the step was.
    #[tokio::test]
    async fn a_build_step_that_is_not_a_local_prebuilt_module_is_refused() {
        let err =
            vet(&project_with(vec![BuildStep::Script(script("make"))], vec![]).await).unwrap_err();
        assert!(matches!(err, Refused::BuildStep { ref canister, .. } if canister == "backend"));
        let text = err.to_string();
        assert!(
            text.contains("not a pre-built module from a file in the project"),
            "{text}"
        );
        assert!(text.contains("(script) make"), "{text}");

        let several = script::Adapter {
            command: script::CommandField::Commands(vec!["make".into(), "cp a b".into()]),
        };
        let err = vet(&project_with(vec![BuildStep::Script(several)], vec![]).await).unwrap_err();
        assert!(err.to_string().contains("(script) make; cp a b"), "{err}");

        let remote = BuildStep::Prebuilt(prebuilt::Adapter {
            source: SourceField::Remote(prebuilt::RemoteSource {
                url: "https://example.com/backend.wasm".into(),
            }),
            sha256: Some("abc".into()),
        });
        let err = vet(&project_with(vec![remote], vec![]).await).unwrap_err();
        assert!(matches!(err, Refused::BuildStep { .. }));
        assert!(
            err.to_string().contains("example.com/backend.wasm"),
            "{err}"
        );

        // One good step does not carry a bad one.
        let err = vet(&project_with(
            vec![local("backend.wasm"), BuildStep::Script(script("make"))],
            vec![],
        )
        .await)
        .unwrap_err();
        assert!(matches!(err, Refused::BuildStep { .. }));
    }

    #[tokio::test]
    async fn a_sync_step_that_is_not_a_local_plugin_is_refused() {
        let err = vet(&project_with(
            vec![local("backend.wasm")],
            vec![SyncStep::Script(script("upload"))],
        )
        .await)
        .unwrap_err();
        assert!(matches!(err, Refused::SyncStep { .. }));
        let text = err.to_string();
        assert!(
            text.contains("not a plugin from a file in the project"),
            "{text}"
        );
        assert!(text.contains("script"), "{text}");

        let err = vet(&project_with(
            vec![local("backend.wasm")],
            vec![plugin("url: https://example.com/plugin.wasm")],
        )
        .await)
        .unwrap_err();
        assert!(matches!(err, Refused::SyncStep { .. }));
        assert!(err.to_string().contains("example.com/plugin.wasm"), "{err}");
    }

    #[tokio::test]
    async fn no_recipe_is_fetched_whatever_it_names() {
        for spec in [
            "file://recipe.hbs",
            "https://example.com/recipe.hbs",
            "@dfinity/rust@v1",
        ] {
            let recipe: Recipe = serde_yaml::from_str(&format!("type: \"{spec}\"")).unwrap();
            let err = match NoRecipes.resolve(&recipe).await {
                Ok(_) => panic!("{spec} was fetched"),
                Err(err) => err.to_string(),
            };
            assert!(err.contains("not fetched"), "{spec}: {err}");
            assert!(err.contains(spec), "{spec}: {err}");
        }
    }

    #[tokio::test]
    async fn a_vetted_loader_refuses_what_vet_refuses() {
        let project = project_with(vec![BuildStep::Script(script("make"))], vec![]).await;
        let loader = Vetted::new(MockProjectLoader::new(project));
        assert!(loader.exists().await.unwrap());
        let err = loader.load().await.unwrap_err();
        assert!(matches!(err, ProjectLoadError::Untrusted { .. }), "{err}");

        let loader = Vetted::new(MockProjectLoader::minimal());
        loader
            .load()
            .await
            .expect("the minimal project is pre-built");
    }
}
