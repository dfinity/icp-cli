use std::sync::Arc;

use crate::{
    Canister,
    canister::build::{Borrowed, Build, BuildError, Module, Params},
    prelude::*,
};
use futures::{StreamExt, stream::FuturesOrdered};
use icp_events::{StepOutcome, TaskOutcome};

use crate::operations::task::{Reporter, Task, TaskReporter};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum BuildOperationError {
    #[snafu(display("failed to create temporary build directory"))]
    TempDir { source: crate::files::FsError },

    #[snafu(transparent)]
    Build { source: BuildError },

    #[snafu(display("build did not produce a wasm output file"))]
    MissingWasmOutput,

    #[snafu(display("failed to read wasm output file"))]
    ReadWasmOutput { source: crate::files::FsError },

    #[snafu(display("failed to copy wasm '{path}' to output file"))]
    CopyWasmOutput {
        source: crate::files::FsError,
        path: PathBuf,
    },

    #[snafu(display("failed to save wasm artifact"))]
    SaveWasmArtifact {
        source: crate::store_artifact::SaveError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to build."))]
pub struct BuildManyError {
    names: Vec<String>,
}

pub async fn build(
    canister_path: &Path,
    canister: &Canister,
    environment: &str,
    task: &TaskReporter,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn crate::store_artifact::Access>,
    files: &dyn crate::files::FileSystem,
) -> Result<(), BuildOperationError> {
    // From `files`, not from this machine: a step writes the module there and
    // the read below comes back through the same seam, so both have to be
    // looking at the same directory.
    let build_dir = files.scratch_dir().await.context(TempDirSnafu)?;
    let wasm_output_path = build_dir.path().join("out.wasm");

    // The module, while the last step to run has named a file of someone
    // else's rather than left one at the output path.
    let mut borrowed: Option<Borrowed> = None;

    let step_count = canister.build.steps.len();
    for (i, step) in canister.build.steps.iter().enumerate() {
        // A step looks for what the steps before it made at the output path,
        // and may change it there, so a borrowed module is copied: the step
        // gets a file of its own, and the one borrowed stays as it was. Only
        // now, when there is a step to want it. The last step's is never
        // copied at all.
        if let Some(module) = borrowed.take() {
            files
                .copy(&module.path, &wasm_output_path)
                .await
                .context(CopyWasmOutputSnafu { path: module.path })?;
        }

        let reporter = task.step(i + 1, step_count, step.to_string());

        let build_result = builder
            .build(
                step,
                &Params {
                    path: canister_path.to_owned(),
                    output: wasm_output_path.to_owned(),
                    environment: environment.to_owned(),
                },
                &reporter,
            )
            .await;

        reporter.done(match &build_result {
            Ok(_) => StepOutcome::Succeeded,
            Err(_) => StepOutcome::Failed,
        });

        if let Module::Borrowed(module) = build_result? {
            borrowed = Some(module);
        }
    }

    // A borrowed module is the artifact where it lies: the store keeps its
    // path, and every canister built from that file shares the one copy.
    if let Some(module) = borrowed {
        return artifacts
            .link(&canister.name, &module.path, &module.sha256)
            .await
            .context(SaveWasmArtifactSnafu);
    }

    if !files.exists(&wasm_output_path).await {
        return MissingWasmOutputSnafu.fail();
    }

    let wasm = files
        .read(&wasm_output_path)
        .await
        .context(ReadWasmOutputSnafu)?;

    artifacts
        .save(&canister.name, &wasm)
        .await
        .context(SaveWasmArtifactSnafu)?;

    Ok(())
}

pub async fn build_many(
    canisters: Vec<(PathBuf, Canister)>,
    environment: &str,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn crate::store_artifact::Access>,
    files: &dyn crate::files::FileSystem,
    reporter: &Reporter,
) -> Result<(), BuildManyError> {
    let mut futs = FuturesOrdered::new();

    for (canister_path, canister) in canisters {
        let task = reporter.task(Task::build(canister.name.clone()));
        let builder = builder.clone();
        let artifacts = artifacts.clone();

        let fut = async move {
            let result = build(
                &canister_path,
                &canister,
                environment,
                &task,
                builder,
                artifacts,
                files,
            )
            .await;

            match &result {
                Ok(()) => task.finish(TaskOutcome::succeeded()),
                Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
            }

            result.map_err(|_| canister.name.clone())
        };
        futs.push_back(fut);
    }

    // Consume the set of futures and collect the failed canister names; the
    // renderer owns displaying each failure's captured output.
    let mut failed: Vec<String> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(name) = res {
            failed.push(name);
        }
    }

    if !failed.is_empty() {
        return BuildManySnafu { names: failed }.fail();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use icp_events::StepReporter;

    use super::*;
    use crate::{
        MockProjectLoader, ProjectLoad,
        files::{FileSystem, FsError, Scratch},
        manifest::{
            adapter::{
                prebuilt::{self, SourceField},
                script,
            },
            canister::{BuildStep, BuildSteps},
        },
        store_artifact::{Access, LookupArtifactError, SaveError},
    };

    const SCRATCH: &str = "/scratch";
    const OUTPUT: &str = "/scratch/out.wasm";

    /// A disk that holds the modules pre-built steps name and the build's
    /// scratch directory, and keeps count of what is copied on it.
    #[derive(Default)]
    struct Disk {
        files: Mutex<HashMap<PathBuf, Vec<u8>>>,
        copied: Mutex<Vec<(PathBuf, PathBuf)>>,
    }

    impl Disk {
        /// A disk with a module at each of `modules`, holding its own path.
        fn with(modules: &[&str]) -> Arc<Self> {
            let disk = Self::default();
            for module in modules {
                disk.files
                    .lock()
                    .unwrap()
                    .insert(module.into(), module.as_bytes().into());
            }
            Arc::new(disk)
        }

        fn copied(&self) -> Vec<(PathBuf, PathBuf)> {
            self.copied.lock().unwrap().clone()
        }

        fn paths(&self) -> Vec<PathBuf> {
            let mut paths: Vec<_> = self.files.lock().unwrap().keys().cloned().collect();
            paths.sort();
            paths
        }
    }

    struct Dir;

    impl Scratch for Dir {
        fn path(&self) -> &Path {
            Path::new(SCRATCH)
        }
    }

    #[derive(Debug, Snafu)]
    #[snafu(display("nothing at '{path}'"))]
    struct Missing {
        path: PathBuf,
    }

    #[async_trait]
    impl FileSystem for Disk {
        async fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| FsError::new(Missing { path: path.into() }))
        }
        async fn copy(&self, from: &Path, to: &Path) -> Result<(), FsError> {
            let contents = self.read(from).await?;
            self.copied.lock().unwrap().push((from.into(), to.into()));
            self.files.lock().unwrap().insert(to.into(), contents);
            Ok(())
        }
        async fn exists(&self, path: &Path) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }
        async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
            Ok(Box::new(Dir))
        }
        async fn write(&self, _: &Path, _: &[u8]) -> Result<(), FsError> {
            unreachable!("the build writes no file of its own making")
        }
        async fn read_to_string(&self, _: &Path) -> Result<String, FsError> {
            unreachable!("the build reads only the module")
        }
        async fn create_dir_all(&self, _: &Path) -> Result<(), FsError> {
            unreachable!("the build makes no directory but its scratch one")
        }
        async fn is_file(&self, _: &Path) -> bool {
            unreachable!("the build asks only whether the output is there")
        }
        async fn is_dir(&self, _: &Path) -> bool {
            unreachable!("the build asks only whether the output is there")
        }
        async fn read_dir(&self, _: &Path) -> Result<Vec<PathBuf>, FsError> {
            unreachable!("the build lists nothing")
        }
        async fn canonicalize(&self, _: &Path) -> Option<PathBuf> {
            unreachable!("the build resolves nothing")
        }
    }

    /// Steps as the two kinds behave: a pre-built step names the module it
    /// was given and touches no file, and a script step appends its command to
    /// whatever is at the output path — or, if the command is `nothing`,
    /// leaves the output as it found it.
    struct Steps(Arc<Disk>);

    #[async_trait]
    impl Build for Steps {
        async fn build(
            &self,
            step: &BuildStep,
            params: &Params,
            _reporter: &StepReporter,
        ) -> Result<Module, BuildError> {
            match step {
                BuildStep::Prebuilt(adapter) => {
                    let SourceField::Local(local) = &adapter.source else {
                        unreachable!("every module here is a local one")
                    };
                    Ok(Module::Borrowed(borrowed(local.path.as_str())))
                }
                BuildStep::Script(adapter) => {
                    let script::CommandField::Command(command) = &adapter.command else {
                        unreachable!("every script here is one command")
                    };
                    if command != "nothing" {
                        let mut files = self.0.files.lock().unwrap();
                        let module = files.entry(params.output.clone()).or_default();
                        module.extend_from_slice(command.as_bytes());
                    }
                    Ok(Module::AtOutput)
                }
            }
        }
    }

    /// What the store was last asked to hold for a canister.
    #[derive(Debug, PartialEq, Eq)]
    enum Kept {
        Saved(Vec<u8>),
        Linked(Borrowed),
    }

    #[derive(Default)]
    struct Store(Mutex<HashMap<String, Kept>>);

    #[async_trait]
    impl Access for Store {
        async fn save(&self, name: &str, wasm: &[u8]) -> Result<(), SaveError> {
            let mut kept = self.0.lock().unwrap();
            kept.insert(name.to_owned(), Kept::Saved(wasm.to_vec()));
            Ok(())
        }
        async fn link(&self, name: &str, path: &Path, sha256: &str) -> Result<(), SaveError> {
            let module = Borrowed {
                path: path.to_owned(),
                sha256: sha256.to_owned(),
            };
            let mut kept = self.0.lock().unwrap();
            kept.insert(name.to_owned(), Kept::Linked(module));
            Ok(())
        }
        async fn lookup(&self, _: &str) -> Result<Vec<u8>, LookupArtifactError> {
            unreachable!("the build looks nothing up")
        }
    }

    fn prebuilt(module: &str) -> BuildStep {
        BuildStep::Prebuilt(prebuilt::Adapter {
            source: SourceField::Local(prebuilt::LocalSource {
                path: module.into(),
            }),
            sha256: None,
        })
    }

    fn script(command: &str) -> BuildStep {
        BuildStep::Script(script::Adapter {
            command: script::CommandField::Command(command.to_owned()),
        })
    }

    /// The module at `path` as a pre-built step names it.
    fn borrowed(path: &str) -> Borrowed {
        Borrowed {
            path: path.into(),
            sha256: format!("sha256 of {path}"),
        }
    }

    /// Build a canister of `steps` on `disk`, and give back what the store
    /// was left holding for it.
    async fn built(steps: Vec<BuildStep>, disk: &Arc<Disk>) -> Result<Kept, BuildOperationError> {
        let mut project = MockProjectLoader::minimal().load().await.unwrap();
        let (path, mut canister) = project.canisters.swap_remove("backend").unwrap();
        canister.build = BuildSteps { steps };

        let store = Arc::new(Store::default());
        build(
            &path,
            &canister,
            "local",
            &TaskReporter::null(),
            Arc::new(Steps(disk.clone())),
            store.clone(),
            disk.as_ref(),
        )
        .await?;

        let kept = store.0.lock().unwrap().remove(&canister.name);
        Ok(kept.expect("a build that succeeds leaves an artifact"))
    }

    /// The whole point: the module a pre-built step names is the artifact
    /// where it lies. Nothing is copied or written, and the disk holds the one
    /// file it held.
    #[tokio::test]
    async fn a_borrowed_module_is_linked_where_it_lies_and_copied_nowhere() {
        let disk = Disk::with(&["/cache/module.wasm"]);

        let kept = built(vec![prebuilt("/cache/module.wasm")], &disk).await;

        assert_eq!(kept.unwrap(), Kept::Linked(borrowed("/cache/module.wasm")));
        assert_eq!(disk.copied(), []);
        assert_eq!(disk.paths(), [Path::new("/cache/module.wasm")]);
    }

    /// A step that follows may change the module, so it gets a copy at the
    /// output path, made once, and what it makes of that copy is what is
    /// saved. The module that was borrowed is as it was.
    #[tokio::test]
    async fn a_borrowed_module_is_copied_for_the_step_that_follows_it() {
        let disk = Disk::with(&["/cache/module.wasm"]);

        let kept = built(
            vec![
                prebuilt("/cache/module.wasm"),
                script("+shrunk"),
                script("nothing"),
                script("+signed"),
            ],
            &disk,
        )
        .await;

        assert_eq!(
            kept.unwrap(),
            Kept::Saved(b"/cache/module.wasm+shrunk+signed".to_vec())
        );
        assert_eq!(
            disk.copied(),
            [("/cache/module.wasm".into(), OUTPUT.into())]
        );
        assert_eq!(
            disk.read(Path::new("/cache/module.wasm")).await.unwrap(),
            b"/cache/module.wasm"
        );
    }

    /// A pre-built step after steps that wrote the output replaces what they
    /// made, as copying over the output did.
    #[tokio::test]
    async fn a_borrowed_module_replaces_what_the_steps_before_it_wrote() {
        let disk = Disk::with(&["/cache/module.wasm"]);
        let kept = built(
            vec![script("scripted"), prebuilt("/cache/module.wasm")],
            &disk,
        )
        .await;
        assert_eq!(kept.unwrap(), Kept::Linked(borrowed("/cache/module.wasm")));
        assert_eq!(disk.copied(), []);

        let disk = Disk::with(&["/cache/module.wasm"]);
        let kept = built(
            vec![
                script("scripted"),
                prebuilt("/cache/module.wasm"),
                script("+shrunk"),
            ],
            &disk,
        )
        .await;
        assert_eq!(
            kept.unwrap(),
            Kept::Saved(b"/cache/module.wasm+shrunk".to_vec())
        );
    }

    /// The last of several pre-built steps is the one linked, and the one
    /// before it is copied only because a step followed it.
    #[tokio::test]
    async fn the_last_borrowed_module_is_the_one_linked() {
        let disk = Disk::with(&["/cache/first.wasm", "/cache/second.wasm"]);

        let kept = built(
            vec![
                prebuilt("/cache/first.wasm"),
                prebuilt("/cache/second.wasm"),
            ],
            &disk,
        )
        .await;

        assert_eq!(kept.unwrap(), Kept::Linked(borrowed("/cache/second.wasm")));
    }

    #[tokio::test]
    async fn steps_that_leave_no_module_anywhere_fail_the_build() {
        let disk = Disk::with(&[]);

        let kept = built(vec![script("nothing")], &disk).await;

        assert!(matches!(kept, Err(BuildOperationError::MissingWasmOutput)));
    }
}
