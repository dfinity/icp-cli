use icp_events::StepReporter;
use snafu::prelude::*;

use crate::{canister::wasm, manifest::adapter::prebuilt::Adapter, prelude::*};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PrebuiltError {
    #[snafu(transparent)]
    Wasm { source: wasm::FetchError },

    #[snafu(display("failed to read wasm file '{path}'"))]
    ReadFile {
        source: crate::files::FsError,
        path: PathBuf,
    },
}

/// The module the step names, read from where [`wasm::Fetch`] found it.
///
/// That file is not the build's: it is the user's own, or an entry in a cache
/// every project shares. So it is read and never written, and the bytes are
/// handed back rather than copied to `params.output`, which would hold the
/// module on disk a second time for no reader but the build itself.
pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    reporter: &StepReporter,
    wasm: &dyn wasm::Fetch,
    files: &dyn crate::files::FileSystem,
) -> Result<Vec<u8>, PrebuiltError> {
    let src = wasm
        .wasm(
            &adapter.source,
            &params.path,
            adapter.sha256.as_deref(),
            reporter,
        )
        .await?;

    files.read(&src).await.context(ReadFileSnafu { path: src })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use camino_tempfile::tempdir;

    use super::*;
    use crate::{
        files::HostFileSystem,
        manifest::adapter::prebuilt::{LocalSource, SourceField},
    };

    /// A [`wasm::Fetch`] that finds a local module where the manifest says.
    struct InPlace;

    #[async_trait]
    impl wasm::Fetch for InPlace {
        async fn wasm(
            &self,
            source: &SourceField,
            base_dir: &Path,
            _sha256: Option<&str>,
            _reporter: &StepReporter,
        ) -> Result<PathBuf, wasm::FetchError> {
            let SourceField::Local(local) = source else {
                unreachable!("every module here is a local one")
            };
            Ok(base_dir.join(&local.path))
        }
    }

    async fn step(dir: &Path) -> Result<Vec<u8>, PrebuiltError> {
        build(
            &Adapter {
                source: SourceField::Local(LocalSource {
                    path: "module.wasm".into(),
                }),
                sha256: None,
            },
            &Params {
                path: dir.to_owned(),
                output: dir.join("out.wasm"),
                environment: "local".to_owned(),
            },
            &StepReporter::null(),
            &InPlace,
            &HostFileSystem,
        )
        .await
    }

    /// The module is handed back from where it was found: nothing is written
    /// at the output path, and the directory holds the one file it held.
    #[tokio::test]
    async fn the_module_is_handed_back_and_not_copied_to_the_output() {
        let dir = tempdir().unwrap();
        let module = dir.path().join("module.wasm");
        std::fs::write(&module, b"module").unwrap();

        assert_eq!(step(dir.path()).await.unwrap(), b"module");

        assert!(!dir.path().join("out.wasm").exists());
        assert_eq!(std::fs::read(&module).unwrap(), b"module");
        assert_eq!(crate::fs::read_dir(dir.path()).unwrap(), [module]);
    }

    /// A module that is not there fails the step that names it, by its path.
    #[tokio::test]
    async fn a_module_that_cannot_be_read_fails_the_step_naming_it() {
        let dir = tempdir().unwrap();

        let err = step(dir.path()).await.unwrap_err();

        assert!(matches!(err, PrebuiltError::ReadFile { .. }));
        let module = dir.path().join("module.wasm");
        assert_eq!(
            err.to_string(),
            format!("failed to read wasm file '{module}'")
        );
    }
}
