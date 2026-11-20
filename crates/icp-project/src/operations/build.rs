use std::sync::Arc;

use crate::{
    Canister,
    canister::build::{Build, BuildError, Module, Params},
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

    #[snafu(display("failed to write wasm output file"))]
    WriteWasmOutput { source: crate::files::FsError },

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

    // The module, while the last step to run has handed it back rather than
    // left it at the output path.
    let mut handed_back: Option<Vec<u8>> = None;

    let step_count = canister.build.steps.len();
    for (i, step) in canister.build.steps.iter().enumerate() {
        // A step looks for what the steps before it made at the output path,
        // so a module that was handed back is written there. Only now, when
        // there is a step to look for it: the last step's is saved as it is,
        // and is never on disk twice.
        if let Some(wasm) = handed_back.take() {
            files
                .write(&wasm_output_path, &wasm)
                .await
                .context(WriteWasmOutputSnafu)?;
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

        if let Module::Bytes(wasm) = build_result? {
            handed_back = Some(wasm);
        }
    }

    let wasm = match handed_back {
        Some(wasm) => wasm,
        None => {
            if !files.exists(&wasm_output_path).await {
                return MissingWasmOutputSnafu.fail();
            }
            files
                .read(&wasm_output_path)
                .await
                .context(ReadWasmOutputSnafu)?
        }
    };

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
        store_artifact::{Access, MockInMemoryArtifactStore},
    };

    const SCRATCH: &str = "/scratch";
    const OUTPUT: &str = "/scratch/out.wasm";

    /// A disk of nothing but the build's scratch directory, which keeps count
    /// of what the operation writes to it.
    #[derive(Default)]
    struct Disk {
        files: Mutex<HashMap<PathBuf, Vec<u8>>>,
        written: Mutex<Vec<PathBuf>>,
    }

    impl Disk {
        fn written(&self) -> Vec<PathBuf> {
            self.written.lock().unwrap().clone()
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
        async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
            self.written.lock().unwrap().push(path.into());
            self.files
                .lock()
                .unwrap()
                .insert(path.into(), contents.into());
            Ok(())
        }
        async fn exists(&self, path: &Path) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }
        async fn scratch_dir(&self) -> Result<Box<dyn Scratch>, FsError> {
            Ok(Box::new(Dir))
        }
        async fn copy(&self, _: &Path, _: &Path) -> Result<(), FsError> {
            unreachable!("the build copies nothing")
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

    /// Steps as the two kinds behave: a pre-built step hands back the module
    /// it names and touches no file, and a script step appends its command to
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
                    Ok(Module::Bytes(local.path.as_str().into()))
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

    /// Build a canister of `steps`, and give back what was saved for it and
    /// the disk it was built on.
    async fn built(steps: Vec<BuildStep>) -> (Result<Vec<u8>, BuildOperationError>, Arc<Disk>) {
        let mut project = MockProjectLoader::minimal().load().await.unwrap();
        let (path, mut canister) = project.canisters.swap_remove("backend").unwrap();
        canister.build = BuildSteps { steps };

        let disk = Arc::new(Disk::default());
        let artifacts = Arc::new(MockInMemoryArtifactStore::new());
        let result = build(
            &path,
            &canister,
            "local",
            &TaskReporter::null(),
            Arc::new(Steps(disk.clone())),
            artifacts.clone(),
            disk.as_ref(),
        )
        .await;

        let saved = match result {
            Ok(()) => Ok(artifacts.lookup(&canister.name).await.unwrap()),
            Err(e) => Err(e),
        };
        (saved, disk)
    }

    /// The whole point: a module the last step hands back goes to the
    /// artifact store and nowhere else, so the build holds it on disk once.
    #[tokio::test]
    async fn a_module_handed_back_by_the_last_step_is_written_nowhere_but_the_store() {
        let (saved, disk) = built(vec![prebuilt("module")]).await;

        assert_eq!(saved.unwrap(), b"module");
        assert_eq!(disk.written(), Vec::<PathBuf>::new());
        assert!(!disk.exists(Path::new(OUTPUT)).await);
    }

    /// A step that follows looks for the module at the output path, so there
    /// it is written, once, and what the step makes of it is what is saved.
    #[tokio::test]
    async fn a_module_handed_back_is_at_the_output_path_for_the_step_that_follows() {
        let (saved, disk) = built(vec![prebuilt("module"), script("+shrunk")]).await;

        assert_eq!(saved.unwrap(), b"module+shrunk");
        assert_eq!(disk.written(), [Path::new(OUTPUT)]);
    }

    /// It is written for the next step alone: steps after that one find the
    /// output as the step before them left it.
    #[tokio::test]
    async fn a_module_handed_back_is_written_once_however_many_steps_follow() {
        let (saved, disk) = built(vec![
            prebuilt("module"),
            script("+shrunk"),
            script("nothing"),
            script("+signed"),
        ])
        .await;

        assert_eq!(saved.unwrap(), b"module+shrunk+signed");
        assert_eq!(disk.written(), [Path::new(OUTPUT)]);
    }

    /// A pre-built step after a step that wrote the output replaces it, as
    /// copying over the output did.
    #[tokio::test]
    async fn a_module_handed_back_replaces_what_the_steps_before_it_wrote() {
        let (saved, disk) = built(vec![script("scripted"), prebuilt("module")]).await;

        assert_eq!(saved.unwrap(), b"module");
        assert_eq!(disk.written(), Vec::<PathBuf>::new());

        let (saved, _) = built(vec![
            script("scripted"),
            prebuilt("module"),
            script("+shrunk"),
        ])
        .await;
        assert_eq!(saved.unwrap(), b"module+shrunk");
    }

    #[tokio::test]
    async fn steps_that_leave_no_module_anywhere_fail_the_build() {
        let (saved, _) = built(vec![script("nothing")]).await;

        assert!(matches!(saved, Err(BuildOperationError::MissingWasmOutput)));
    }
}
