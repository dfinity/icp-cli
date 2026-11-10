//! Holding a project someone else handed over to what it may read and run.
//!
//! The CLI trusts a project the way it trusts the user's own checkout: a
//! build step may run any command, a sync step any script, and a manifest
//! may name any path on the machine. A project from elsewhere — an archive
//! an agent passed to a deploy service, a checkout the user has not read —
//! may not. Two things hold it:
//!
//! - what it reads goes through a filesystem confined to the project root —
//!   on a host, [`Confined`](crate::files::Confined) — so that every path a
//!   manifest names is under the root and reached through no symlink. The
//!   id and artifact stores under `.icp` are held to the same filesystem;
//! - what it runs is on a list, stated as what is allowed so that a kind of
//!   step added later is refused until someone decides otherwise: a build
//!   step is a pre-built module, a sync step is a plugin, and the file either
//!   names is a regular file that filesystem will give.
//!
//! What the tool keeps under `.icp` is not part of the project: it is state
//! an earlier run wrote and a later one reads back as its own, a network's
//! descriptor among it, which says where that network's files are. Whoever
//! takes a project from elsewhere discards any `.icp` that came with it, and
//! nothing here vets what one holds.
//!
//! A plugin's runtime opens the plugin, and each file and directory the step
//! hands it, by path, after [`vet`] has looked. So those have the rule but
//! not the guarantee a read through the confined filesystem has: a tree
//! rewritten between the look and the open is not caught.
//!
//! A module or plugin named by URL is not on the list, and the steps a recipe
//! renders to are held to it like any other. A checksum is not asked for: the
//! same untrusted hand wrote it and the module. [`vet`] applies the list to a
//! loaded [`Project`], and [`Vetted`] wraps a loader so that every load is
//! vetted before the project is handed out.

use std::sync::Arc;

use async_trait::async_trait;
use snafu::{ResultExt, Snafu, ensure};

use crate::{
    Project, ProjectLoad, ProjectLoadError, UntrustedSnafu,
    files::FileSystem,
    manifest::{
        adapter::{
            plugin::NamedPaths,
            prebuilt::{LocalSource, SourceField},
        },
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
        "canister '{canister}' names '{path}', which is not a regular file in the project: an \
         untrusted project runs only a file under its root, reached through no symlink"
    ))]
    NotAFile { canister: String, path: PathBuf },

    #[snafu(display(
        "canister '{canister}' gives its sync plugin '{path}', which is not a regular file or \
         directory in the project: an untrusted project hands a plugin only what is under its \
         root, reached through no symlink"
    ))]
    PluginInput { canister: String, path: PathBuf },
}

/// The file `step` names, if it is the one build step an untrusted project
/// may have: a pre-built module from a file.
fn local_module(step: &BuildStep) -> Option<&LocalSource> {
    match step {
        BuildStep::Prebuilt(adapter) => local_source(&adapter.source),
        BuildStep::Script(_) => None,
    }
}

/// The file `step` names, if it is the one sync step an untrusted project
/// may have: a plugin from a file.
fn local_plugin(step: &SyncStep) -> Option<&LocalSource> {
    match step {
        SyncStep::Plugin(adapter) => local_source(&adapter.source),
        SyncStep::Script(_) => None,
    }
}

fn local_source(source: &SourceField) -> Option<&LocalSource> {
    match source {
        SourceField::Local(local) => Some(local),
        SourceField::Remote(_) => None,
    }
}

/// Hold a loaded project to the list.
///
/// `files` is the filesystem the project is confined to, and it is what
/// says whether the file a step names is one: under the root, through no
/// symlink, and a regular file. A plugin is opened by its runtime rather
/// than read through `files`, and a read of a pipe nobody writes to waits
/// forever, so the question is asked here, before anything opens the file.
/// The same is asked of what a plugin step hands its plugin, which the
/// runtime also opens itself: a `dirs` entry is a directory, and a `files`
/// entry a regular file or a directory.
pub async fn vet(project: &Project, files: &dyn FileSystem) -> Result<(), Refused> {
    for (_, (canister_dir, canister)) in &project.canisters {
        let build = canister.build.steps.iter().map(|step| {
            local_module(step).ok_or_else(|| {
                BuildStepSnafu {
                    canister: &canister.name,
                    step: one_line(step),
                }
                .build()
            })
        });
        let sync = canister.sync.steps.iter().map(|step| {
            local_plugin(step).ok_or_else(|| {
                SyncStepSnafu {
                    canister: &canister.name,
                    step: one_line(step),
                }
                .build()
            })
        });
        for local in build.chain(sync) {
            let path = canister_dir.join(&local?.path);
            ensure!(
                files.is_file(&path).await,
                NotAFileSnafu {
                    canister: &canister.name,
                    path,
                }
            );
        }
        for step in &canister.sync.steps {
            let SyncStep::Plugin(adapter) = step else {
                continue;
            };
            let dirs = adapter.dirs.iter().flat_map(NamedPaths::entries);
            let named = adapter.files.iter().flat_map(NamedPaths::entries);
            let inputs = dirs
                .map(|entry| (entry.path, false))
                .chain(named.map(|entry| (entry.path, true)));
            for (declared, may_be_a_file) in inputs {
                let path = canister_dir.join(declared);
                let held =
                    (may_be_a_file && files.is_file(&path).await) || files.is_dir(&path).await;
                ensure!(
                    held,
                    PluginInputSnafu {
                        canister: &canister.name,
                        path,
                    }
                );
            }
        }
    }
    Ok(())
}

/// A step as it displays, on one line: its kind, then the lines after the
/// first run together with `;` between.
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
    use std::collections::HashSet;

    use super::*;
    use crate::{
        MockProjectLoader,
        files::{FsError, Scratch},
        manifest::{
            adapter::{prebuilt, script},
            canister::{BuildSteps, SyncSteps},
        },
    };

    /// A filesystem that knows which paths are regular files, which is all
    /// the vet asks of one.
    struct Disk(HashSet<PathBuf>);

    #[async_trait]
    impl FileSystem for Disk {
        async fn is_file(&self, path: &Path) -> bool {
            self.0.contains(path)
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
            unreachable!("the vet asks only for a regular file")
        }
        async fn is_dir(&self, _: &Path) -> bool {
            false
        }
        async fn read_dir(&self, _: &Path) -> Result<Vec<PathBuf>, FsError> {
            unreachable!("the vet reads nothing")
        }
        async fn canonicalize(&self, _: &Path) -> Option<PathBuf> {
            unreachable!("the vet asks only for a regular file")
        }
        async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
            unreachable!("the vet writes nothing")
        }
    }

    /// The minimal project is at `/project`, and this is its disk: a module
    /// and a plugin.
    fn disk() -> Arc<Disk> {
        Arc::new(Disk(HashSet::from([
            "/project/backend.wasm".into(),
            "/project/p.wasm".into(),
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

    /// What a step names must be a regular file the filesystem will give,
    /// for a module and for a plugin alike.
    #[tokio::test]
    async fn what_a_step_names_must_be_a_regular_file() {
        let err = vetted(vec![local("missing.wasm")], vec![])
            .await
            .unwrap_err();
        assert!(
            matches!(err, Refused::NotAFile { ref path, .. } if path == "/project/missing.wasm"),
            "{err}"
        );
        assert!(err.to_string().contains("not a regular file"), "{err}");

        let err = vetted(vec![local("backend.wasm")], vec![plugin("path: gone.wasm")])
            .await
            .unwrap_err();
        assert!(matches!(err, Refused::NotAFile { .. }), "{err}");
    }

    /// On a real disk, through the filesystem the CLI confines an untrusted
    /// project to: a module of the project's own is accepted, with the root
    /// spelled through a link as a macOS temp dir is, and one outside the
    /// root, behind a link wherever it leads, or not a regular file is
    /// refused, the pipe before anything tries to read it.
    #[cfg(all(unix, feature = "host"))]
    #[tokio::test]
    async fn on_disk_a_step_names_a_regular_file_under_the_root_through_no_link() {
        use crate::files::Confined;
        use std::os::unix::fs::symlink;

        let dir = camino_tempfile::tempdir().unwrap();
        let real = dir.path().join("project");
        std::fs::create_dir(&real).unwrap();
        let root = dir.path().join("alias");
        symlink(&real, &root).unwrap();
        std::fs::write(root.join("backend.wasm"), b"module").unwrap();
        std::fs::write(dir.path().join("secret.wasm"), b"secret").unwrap();
        symlink(root.join("backend.wasm"), root.join("in.wasm")).unwrap();
        symlink(dir.path().join("secret.wasm"), root.join("out.wasm")).unwrap();
        symlink(dir.path(), root.join("outdir")).unwrap();
        std::fs::create_dir(root.join("dir.wasm")).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(root.join("pipe.wasm"))
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo");
        let files = Confined::new(&root).unwrap();

        let mut project = project_with(vec![local("backend.wasm")], vec![]).await;
        project.dir = root.clone();
        project.canisters.get_mut("backend").unwrap().0 = root.clone();
        vet(&project, &files)
            .await
            .expect("the project's own module");

        for path in [
            "../secret.wasm",
            "in.wasm",
            "out.wasm",
            "outdir/secret.wasm",
            "dir.wasm",
            "pipe.wasm",
        ] {
            project.canisters.get_mut("backend").unwrap().1.build = BuildSteps {
                steps: vec![local(path)],
            };
            let err = vet(&project, &files).await.unwrap_err();
            assert!(matches!(err, Refused::NotAFile { .. }), "{path}: {err}");
        }

        // What a plugin is handed is held the same way: its runtime opens
        // each entry itself, and would wait on the pipe.
        std::fs::write(root.join("p.wasm"), b"plugin").unwrap();
        std::fs::write(root.join("seed.txt"), b"seed").unwrap();
        std::fs::create_dir(root.join("assets")).unwrap();
        let with_inputs = |inputs: &str| SyncSteps {
            steps: vec![plugin(&format!("path: p.wasm\n{inputs}"))],
        };
        let canister = &mut project.canisters.get_mut("backend").unwrap().1;
        canister.build = BuildSteps {
            steps: vec![local("backend.wasm")],
        };
        canister.sync = with_inputs("files:\n  seed: seed.txt\n  assets: assets");
        vet(&project, &files)
            .await
            .expect("the project's own files");
        for inputs in [
            "files:\n  seed: pipe.wasm",
            "files:\n  seed: out.wasm",
            "files:\n  seed: ../secret.wasm",
            "files:\n  seed: gone",
            "dirs: [outdir]",
            "dirs: [seed.txt]",
        ] {
            project.canisters.get_mut("backend").unwrap().1.sync = with_inputs(inputs);
            let err = vet(&project, &files).await.unwrap_err();
            assert!(
                matches!(err, Refused::PluginInput { .. }),
                "{inputs}: {err}"
            );
        }
    }
}
