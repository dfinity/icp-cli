use tokio::sync::mpsc::Sender;

use crate::manifest::adapter::script::Adapter;

use super::Params;

use super::super::script::{ScriptError, execute};

pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
) -> Result<(), ScriptError> {
    execute(
        adapter,
        params.path.as_ref(),
        &[("ICP_WASM_OUTPUT_PATH", params.output.as_ref())],
        stdio,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Read;

    use camino_tempfile::NamedUtf8TempFile;

    use crate::manifest::adapter::script::{Adapter, CommandField};

    #[tokio::test]
    async fn single_command() {
        // Create temporary file
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = Adapter {
            command: CommandField::Command(format!(
                "echo test > '{}' && echo '{}'",
                f.path(),
                f.path()
            )),
        };

        build(
            &v,
            &Params {
                path: "/".into(),
                output: "/".into(),
            },
            None,
        )
        .await
        .expect("failed to build script step");

        // Verify command ran
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, "test\n".to_string());
    }

    #[tokio::test]
    async fn multiple_commands() {
        // Create temporary file
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = Adapter {
            command: CommandField::Commands(vec![
                format!("echo cmd-1 >> '{}'", f.path()),
                format!("echo cmd-2 >> '{}'", f.path()),
                format!("echo cmd-3 >> '{}'", f.path()),
                format!("echo '{}'", f.path()),
            ]),
        };

        build(
            &v,
            &Params {
                path: "/".into(),
                output: "/".into(),
            },
            None,
        )
        .await
        .expect("failed to build script step");

        // Verify command ran
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, "cmd-1\ncmd-2\ncmd-3\n".to_string());
    }

    #[tokio::test]
    async fn invalid_command() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("".into()),
        };

        let out = build(
            &v,
            &Params {
                path: "/".into(),
                output: "/".into(),
            },
            None,
        )
        .await;

        // Assert failure
        if out.is_ok() {
            panic!("expected invalid command to fail");
        }
    }

    #[tokio::test]
    async fn failed_unknown_command() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("unknown-command".into()),
        };

        let out = build(
            &v,
            &Params {
                path: "/".into(),
                output: "/".into(),
            },
            None,
        )
        .await;

        // Assert failure
        if out.is_ok() {
            panic!("expected unknown command to fail");
        }
    }

    #[tokio::test]
    async fn failed_command_error_status() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("exit 1".into()),
        };

        let out = build(
            &v,
            &Params {
                path: "/".into(),
                output: "/".into(),
            },
            None,
        )
        .await;

        // Assert failure
        assert!(out.is_err());
    }
}
