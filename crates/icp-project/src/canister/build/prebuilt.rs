use icp_events::StepReporter;
use sha2::{Digest, Sha256};
use snafu::prelude::*;

use crate::{canister::wasm, manifest::adapter::prebuilt::Adapter, prelude::*};

use super::{Borrowed, Params};

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

/// The module the step names, where [`wasm::Fetch`] found it.
///
/// That file is the user's own, or an entry in a cache every project shares,
/// and it is the step's result as it stands: it is not copied to
/// `params.output`. It is read once, through `files`, so that a file the
/// project's filesystem would not hand over fails the step that names it, and
/// for the hash that tells a later reader whether the file is still the one
/// that was built from.
pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    reporter: &StepReporter,
    wasm: &dyn wasm::Fetch,
    files: &dyn crate::files::FileSystem,
) -> Result<Borrowed, PrebuiltError> {
    let path = wasm
        .wasm(
            &adapter.source,
            &params.path,
            adapter.sha256.as_deref(),
            reporter,
        )
        .await?;

    let bytes = files
        .read(&path)
        .await
        .context(ReadFileSnafu { path: &path })?;
    let sha256 = hex::encode(Sha256::digest(&bytes));

    Ok(Borrowed { path, sha256 })
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

    async fn step(dir: &Path) -> Result<Borrowed, PrebuiltError> {
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

    /// The step's result is the file it was pointed at, with the hash of what
    /// is in it: nothing is written at the output path, and the directory
    /// holds the one file it held.
    #[tokio::test]
    async fn the_module_is_named_where_it_lies_and_not_copied_to_the_output() {
        let dir = tempdir().unwrap();
        let module = dir.path().join("module.wasm");
        std::fs::write(&module, b"abc").unwrap();

        let borrowed = step(dir.path()).await.unwrap();

        assert_eq!(borrowed.path, module);
        // FIPS 180-2, appendix B.1: the SHA-256 of the three bytes "abc".
        assert_eq!(
            borrowed.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(!dir.path().join("out.wasm").exists());
        assert_eq!(std::fs::read(&module).unwrap(), b"abc");
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
