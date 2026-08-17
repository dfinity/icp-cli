use icp_events::OutputWriter;

use crate::manifest::adapter::script::Adapter;

use super::Params;

use super::super::script::{ScriptError, execute};

pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<OutputWriter>,
) -> Result<(), ScriptError> {
    execute(
        adapter,
        params.path.as_ref(),
        &[
            ("ICP_WASM_OUTPUT_PATH", params.output.as_ref()),
            ("ICP_CLI_ENVIRONMENT", &params.environment),
        ],
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
    use crate::prelude::LOCAL;

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
                environment: LOCAL.to_owned(),
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

    /// A build step streams its output through an [`OutputWriter`], so what the
    /// subprocess printed can be observed as events without a terminal anywhere in
    /// the picture.
    #[tokio::test]
    async fn command_output_is_reported_through_the_writer() {
        use std::sync::Arc;

        use icp_events::{Event, RecordingSink, Reporter, TaskKind};

        let sink = Arc::new(RecordingSink::new());
        let mut task = Reporter::new(sink.clone()).task(
            TaskKind::Steps {
                output_label: "Build".to_owned(),
            },
            "backend",
        );
        task.begin_step("step 1 of 1");

        let adapter = Adapter {
            command: CommandField::Command("echo streamed-line".to_owned()),
        };

        build(
            &adapter,
            &Params {
                path: "/".into(),
                output: "/".into(),
                environment: LOCAL.to_owned(),
            },
            Some(task.output()),
        )
        .await
        .expect("failed to build script step");

        let lines: Vec<String> = sink
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::StepOutput { line, .. } => Some(line),
                _ => None,
            })
            .collect();
        assert_eq!(lines, vec!["streamed-line".to_owned()]);

        // The same lines are kept for replay if the step turns out to have failed.
        assert_eq!(
            task.recorded_steps()[0].lines,
            vec!["streamed-line".to_owned()]
        );
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
                environment: LOCAL.to_owned(),
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
    async fn environment_variables() {
        // Create temporary files, one to write the variables to and one to serve
        // as the wasm output path
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");
        let out_wasm = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = Adapter {
            command: CommandField::Command(format!(
                r#"echo "$ICP_CLI_ENVIRONMENT $ICP_WASM_OUTPUT_PATH" > '{}'"#,
                f.path()
            )),
        };

        build(
            &v,
            &Params {
                path: "/".into(),
                output: out_wasm.path().to_owned(),
                environment: "staging".to_owned(),
            },
            None,
        )
        .await
        .expect("failed to build script step");

        // Verify the variables reached the command
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, format!("staging {}\n", out_wasm.path()));
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
                environment: LOCAL.to_owned(),
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
                environment: LOCAL.to_owned(),
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
                environment: LOCAL.to_owned(),
            },
            None,
        )
        .await;

        // Assert failure
        assert!(out.is_err());
    }
}
