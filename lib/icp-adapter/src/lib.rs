use async_trait::async_trait;
use camino::Utf8PathBuf;
use motoko::MotokoAdapterCompileError;
use rust::RustAdapterCompileError;
use script::ScriptAdapterCompileError;
use snafu::Snafu;

pub mod motoko;
pub mod rust;
pub mod script;

#[async_trait]
pub trait Adapter {
    async fn compile(&self, path: Utf8PathBuf) -> Result<(), AdapterCompileError>;
}

#[derive(Debug, Snafu)]
pub enum AdapterCompileError {
    #[snafu(transparent)]
    Rust { source: RustAdapterCompileError },

    #[snafu(transparent)]
    Motoko { source: MotokoAdapterCompileError },

    #[snafu(transparent)]
    Script { source: ScriptAdapterCompileError },
}
