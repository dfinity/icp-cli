//! Holding a project someone else handed over to what it may run.
//!
//! The CLI trusts a project the way it trusts the user's own checkout: a
//! build step may run any command, a sync step any script, and either may
//! take its module or plugin from anywhere on the machine. A project from
//! elsewhere — an archive an agent passed to a deploy service, a checkout the
//! user has not read — may not. The line is between reading and running.
//! Reading is not restricted: the loader reads argument files, environment
//! files, manifests and recipe templates from wherever the manifest points,
//! and a plugin reads the files it declares, through the ordinary filesystem.
//! What runs is held to a list, stated as what is allowed so that a kind of
//! step added later is refused until someone decides otherwise:
//!
//! - a build step is a pre-built module ([`build_step_allowed`]);
//! - a sync step is a plugin ([`sync_step_allowed`]);
//! - the module or plugin a step names is a file within the project: its
//!   path, with every link followed and every `..` applied, lies under the
//!   project root resolved the same way ([`vet`]).
//!
//! A recipe is read and rendered as the project loads, and the steps it
//! produces are on the list or not like any other. A module or plugin named
//! by URL is not on it: a download at build time is what a bundle has already
//! done. The loaded [`Project`] shows every step, so the list is applied to
//! it: [`vet`] holds a project to it, and [`Vetted`] wraps a loader so that
//! every load is vetted before the project is handed out. A consumer
//! composing its own host wraps its loader the same way:
//!
//! ```ignore
//! let project = Vetted::new(ProjectLoadImpl { project_root_locate, recipe, files: files.clone() }, files);
//! ```
//!
//! A checksum on a module is not asked for. It is the bundler's promise about
//! its own archive, and the same untrusted hand wrote both, so it says nothing
//! about what the project runs.

use std::sync::Arc;

use async_trait::async_trait;
use snafu::{ResultExt, Snafu};

use crate::{
    Project, ProjectLoad, ProjectLoadError, UntrustedSnafu,
    files::FileSystem,
    manifest::{
        adapter::prebuilt::SourceField,
        canister::{BuildStep, SyncStep},
    },
    prelude::*,
};

/// Why a loaded project may not be acted on as untrusted.
#[derive(Debug, Snafu)]
pub enum Refused {
    #[snafu(display(
        "canister '{canister}' has a build step that is not a pre-built module, which is all an \
         untrusted project may have: {step}"
    ))]
    BuildStep { canister: String, step: String },

    #[snafu(display(
        "canister '{canister}' has a sync step that is not a plugin, which is all an untrusted \
         project may have: {step}"
    ))]
    SyncStep { canister: String, step: String },

    #[snafu(display(
        "canister '{canister}' names '{path}', which resolves to '{resolved}', outside the \
         project at '{root}': an untrusted project runs only what is within it"
    ))]
    OutsidePath {
        canister: String,
        path: PathBuf,
        resolved: PathBuf,
        root: PathBuf,
    },

    #[snafu(display("canister '{canister}' names '{path}', which does not resolve to a file"))]
    UnresolvedPath { canister: String, path: PathBuf },

    #[snafu(display("the project root '{root}' does not resolve to a directory"))]
    UnresolvedRoot { root: PathBuf },
}

/// Whether `step` is the one build step an untrusted project may have: a
/// pre-built module from a file. A script is a host subprocess, and a module
/// fetched from a URL is a download on the strength of an untrusted manifest.
pub fn build_step_allowed(step: &BuildStep) -> bool {
    matches!(step, BuildStep::Prebuilt(adapter) if matches!(adapter.source, SourceField::Local(_)))
}

/// Whether `step` is the one sync step an untrusted project may have: a
/// plugin from a file. It runs sandboxed either way; it is a script and a
/// download that are not on the list.
pub fn sync_step_allowed(step: &SyncStep) -> bool {
    matches!(step, SyncStep::Plugin(adapter) if matches!(adapter.source, SourceField::Local(_)))
}

/// Hold a loaded project to the list: every step [`build_step_allowed`] or
/// [`sync_step_allowed`], and the file each names within the project.
///
/// Within is decided on the real paths: the file's and the root's, each with
/// every link followed and every `..` applied by `files`, so a link out of
/// the project is seen for where it leads and a link within it is let be. A
/// path that does not resolve is refused too, since nothing can be said
/// about where it would lead.
pub async fn vet(project: &Project, files: &dyn FileSystem) -> Result<(), Refused> {
    let root = files
        .canonicalize(&project.dir)
        .await
        .ok_or_else(|| UnresolvedRootSnafu { root: &project.dir }.build())?;

    for (_, (canister_dir, canister)) in &project.canisters {
        for step in &canister.build.steps {
            let local = match step {
                BuildStep::Prebuilt(adapter) if build_step_allowed(step) => match &adapter.source {
                    SourceField::Local(local) => local,
                    SourceField::Remote(_) => unreachable!("allowed is local"),
                },
                _ => {
                    return BuildStepSnafu {
                        canister: &canister.name,
                        step: one_line(step),
                    }
                    .fail();
                }
            };
            within(files, &root, canister, &canister_dir.join(&local.path)).await?;
        }
        for step in &canister.sync.steps {
            let local = match step {
                SyncStep::Plugin(adapter) if sync_step_allowed(step) => match &adapter.source {
                    SourceField::Local(local) => local,
                    SourceField::Remote(_) => unreachable!("allowed is local"),
                },
                _ => {
                    return SyncStepSnafu {
                        canister: &canister.name,
                        step: one_line(step),
                    }
                    .fail();
                }
            };
            within(files, &root, canister, &canister_dir.join(&local.path)).await?;
        }
    }
    Ok(())
}

/// Refuse `path` unless it resolves to somewhere under the resolved `root`.
async fn within(
    files: &dyn FileSystem,
    root: &Path,
    canister: &crate::Canister,
    path: &Path,
) -> Result<(), Refused> {
    let resolved = files.canonicalize(path).await.ok_or_else(|| {
        UnresolvedPathSnafu {
            canister: &canister.name,
            path,
        }
        .build()
    })?;
    if !resolved.starts_with(root) {
        return OutsidePathSnafu {
            canister: &canister.name,
            path,
            resolved,
            root,
        }
        .fail();
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

/// A [`ProjectLoad`] that hands out only a project [`vet`] accepts.
pub struct Vetted<L> {
    loader: L,
    files: Arc<dyn FileSystem>,
}

impl<L> Vetted<L> {
    /// `files` is what the paths are resolved through: the filesystem the
    /// project is on.
    pub fn new(loader: L, files: Arc<dyn FileSystem>) -> Self {
        Self { loader, files }
    }
}

#[async_trait]
impl<L: ProjectLoad> ProjectLoad for Vetted<L> {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        let project = self.loader.load().await?;
        vet(&project, self.files.as_ref())
            .await
            .context(UntrustedSnafu)?;
        Ok(project)
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        self.loader.exists().await
    }

    fn member_dir(&self) -> Option<PathBuf> {
        self.loader.member_dir()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{
        MockProjectLoader,
        files::{FsError, Scratch},
        manifest::{
            adapter::{prebuilt, script},
            canister::{BuildSteps, SyncSteps},
        },
    };

    /// A filesystem that resolves the paths it is told to and nothing else,
    /// standing in for links and `..` on disk: the vet asks for nothing but
    /// where a path really is.
    struct Resolves(HashMap<PathBuf, PathBuf>);

    #[async_trait]
    impl FileSystem for Resolves {
        async fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
            self.0.get(path).cloned()
        }
        async fn read(&self, _: &Path) -> Result<Vec<u8>, FsError> {
            unreachable!("the vet reads nothing")
        }
        async fn read_to_string(&self, _: &Path) -> Result<String, FsError> {
            unreachable!("the vet reads nothing")
        }
        async fn write(&self, _: &Path, _: &[u8]) -> Result<(), FsError> {
            unreachable!("the vet writes nothing")
        }
        async fn create_dir_all(&self, _: &Path) -> Result<(), FsError> {
            unreachable!("the vet writes nothing")
        }
        async fn copy(&self, _: &Path, _: &Path) -> Result<(), FsError> {
            unreachable!("the vet writes nothing")
        }
        async fn exists(&self, _: &Path) -> bool {
            unreachable!("the vet looks at nothing but where a path is")
        }
        async fn is_file(&self, _: &Path) -> bool {
            unreachable!("the vet looks at nothing but where a path is")
        }
        async fn is_dir(&self, _: &Path) -> bool {
            unreachable!("the vet looks at nothing but where a path is")
        }
        async fn read_dir(&self, _: &Path) -> Result<Vec<PathBuf>, FsError> {
            unreachable!("the vet reads nothing")
        }
        async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
            unreachable!("the vet writes nothing")
        }
    }

    /// The minimal project is at `/project`, and this is its disk: a module
    /// of its own, a plugin, a link out to `/elsewhere/secret.wasm`, a link
    /// within to the module, and a climb that lands outside.
    fn disk() -> Arc<Resolves> {
        Arc::new(Resolves(HashMap::from([
            ("/project".into(), "/project".into()),
            (
                "/project/backend.wasm".into(),
                "/project/backend.wasm".into(),
            ),
            ("/project/p.wasm".into(), "/project/p.wasm".into()),
            ("/project/out.wasm".into(), "/elsewhere/secret.wasm".into()),
            ("/project/in.wasm".into(), "/project/backend.wasm".into()),
            (
                "/project/../elsewhere/secret.wasm".into(),
                "/elsewhere/secret.wasm".into(),
            ),
        ])))
    }

    async fn project_with(build: Vec<BuildStep>, sync: Vec<SyncStep>) -> Project {
        let mut project = MockProjectLoader::minimal().load().await.unwrap();
        let (_, canister) = project.canisters.get_mut("backend").unwrap();
        canister.build = BuildSteps { steps: build };
        canister.sync = SyncSteps { steps: sync };
        project
    }

    async fn vetted(build: Vec<BuildStep>, sync: Vec<SyncStep>) -> Result<(), Refused> {
        vet(&project_with(build, sync).await, disk().as_ref()).await
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
        vetted(vec![local("backend.wasm")], vec![plugin("path: p.wasm")])
            .await
            .expect("pre-built from the project, plugin from the project");
    }

    /// A script step and a remote module are the two other build steps there
    /// are. Neither is on the list, and the message says what the list is
    /// and what the step was.
    #[tokio::test]
    async fn a_build_step_that_is_not_a_local_prebuilt_module_is_refused() {
        let err = vetted(vec![BuildStep::Script(script("make"))], vec![])
            .await
            .unwrap_err();
        assert!(matches!(err, Refused::BuildStep { ref canister, .. } if canister == "backend"));
        let text = err.to_string();
        assert!(text.contains("not a pre-built module"), "{text}");
        assert!(text.contains("(script) make"), "{text}");

        let several = script::Adapter {
            command: script::CommandField::Commands(vec!["make".into(), "cp a b".into()]),
        };
        let err = vetted(vec![BuildStep::Script(several)], vec![])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("(script) make; cp a b"), "{err}");

        let remote = BuildStep::Prebuilt(prebuilt::Adapter {
            source: SourceField::Remote(prebuilt::RemoteSource {
                url: "https://example.com/backend.wasm".into(),
            }),
            sha256: Some("abc".into()),
        });
        let err = vetted(vec![remote], vec![]).await.unwrap_err();
        assert!(matches!(err, Refused::BuildStep { .. }));
        assert!(
            err.to_string().contains("example.com/backend.wasm"),
            "{err}"
        );

        // One good step does not carry a bad one.
        let err = vetted(
            vec![local("backend.wasm"), BuildStep::Script(script("make"))],
            vec![],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Refused::BuildStep { .. }));
    }

    #[tokio::test]
    async fn a_sync_step_that_is_not_a_local_plugin_is_refused() {
        let err = vetted(
            vec![local("backend.wasm")],
            vec![SyncStep::Script(script("upload"))],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Refused::SyncStep { .. }));
        let text = err.to_string();
        assert!(text.contains("not a plugin"), "{text}");
        assert!(text.contains("script"), "{text}");

        let err = vetted(
            vec![local("backend.wasm")],
            vec![plugin("url: https://example.com/plugin.wasm")],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Refused::SyncStep { .. }));
        assert!(err.to_string().contains("example.com/plugin.wasm"), "{err}");
    }

    /// Where a path leads is what counts, not how it is spelled: a link out
    /// is refused for where it lands, a link within is fine, and a climb
    /// out is refused.
    #[tokio::test]
    async fn the_file_a_step_names_is_held_to_where_it_resolves() {
        vetted(vec![local("in.wasm")], vec![])
            .await
            .expect("a link within the project");

        for path in ["out.wasm", "../elsewhere/secret.wasm"] {
            let err = vetted(vec![local(path)], vec![]).await.unwrap_err();
            assert!(
                matches!(err, Refused::OutsidePath { ref resolved, ref root, .. }
                    if resolved == "/elsewhere/secret.wasm" && root == "/project"),
                "{path}: {err}"
            );
            assert!(
                err.to_string().contains("outside the project"),
                "{path}: {err}"
            );
        }

        let err = vetted(vec![local("backend.wasm")], vec![plugin("path: out.wasm")])
            .await
            .unwrap_err();
        assert!(matches!(err, Refused::OutsidePath { .. }), "{err}");

        let err = vetted(vec![local("missing.wasm")], vec![])
            .await
            .unwrap_err();
        assert!(matches!(err, Refused::UnresolvedPath { .. }), "{err}");
        assert!(err.to_string().contains("missing.wasm"), "{err}");
    }

    #[tokio::test]
    async fn a_vetted_loader_refuses_what_vet_refuses() {
        let project = project_with(vec![BuildStep::Script(script("make"))], vec![]).await;
        let loader = Vetted::new(MockProjectLoader::new(project), disk());
        assert!(loader.exists().await.unwrap());
        let err = loader.load().await.unwrap_err();
        assert!(matches!(err, ProjectLoadError::Untrusted { .. }), "{err}");

        let loader = Vetted::new(MockProjectLoader::minimal(), disk());
        loader
            .load()
            .await
            .expect("the minimal project is pre-built from its own file");
    }

    /// On a real disk the links are followed by the host, which is what the
    /// CLI resolves through.
    #[cfg(all(unix, feature = "host"))]
    #[tokio::test]
    async fn on_disk_a_link_is_followed_to_where_it_leads() {
        use crate::files::HostFileSystem;

        let dir = camino_tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("backend.wasm"), b"module").unwrap();
        std::fs::write(dir.path().join("secret.wasm"), b"secret").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.wasm"), root.join("out.wasm")).unwrap();
        std::os::unix::fs::symlink(root.join("backend.wasm"), root.join("in.wasm")).unwrap();

        let mut project = project_with(vec![local("in.wasm")], vec![]).await;
        project.dir = root.clone();
        project.canisters.get_mut("backend").unwrap().0 = root.clone();
        vet(&project, &HostFileSystem)
            .await
            .expect("a link to the project's own module");

        project.canisters.get_mut("backend").unwrap().1.build = BuildSteps {
            steps: vec![local("out.wasm")],
        };
        let err = vet(&project, &HostFileSystem).await.unwrap_err();
        assert!(matches!(err, Refused::OutsidePath { .. }), "{err}");
    }
}
