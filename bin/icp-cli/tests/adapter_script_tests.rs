use std::io::Read;

use camino_tempfile::NamedUtf8TempFile;
use icp_adapter::{
    Adapter, AdapterCompileError,
    script::{CommandField, ScriptAdapter, ScriptAdapterCompileError},
};

#[tokio::test]
async fn single_command() {
    // Create temporary file
    let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

    // Define adapter
    let v = ScriptAdapter {
        command: CommandField::Command(format!("sh -c 'echo test > {}'", f.path())),
    };

    // Invoke adapter
    v.compile("/".into()).await.expect("unexpected failure");

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
    let v = ScriptAdapter {
        command: CommandField::Commands(vec![
            format!("sh -c 'echo cmd-1 >> {}'", f.path()),
            format!("sh -c 'echo cmd-2 >> {}'", f.path()),
            format!("sh -c 'echo cmd-3 >> {}'", f.path()),
        ]),
    };

    // Invoke adapter
    v.compile("/".into()).await.expect("unexpected failure");

    // Verify command ran
    let mut out = String::new();

    f.read_to_string(&mut out)
        .expect("failed to read temporary file");

    assert_eq!(out, "cmd-1\ncmd-2\ncmd-3\n".to_string());
}

#[tokio::test]
async fn invalid_command() {
    // Define adapter
    let v = ScriptAdapter {
        command: CommandField::Command("".into()),
    };

    // Invoke adapter
    let out = v.compile("/".into()).await;

    // Assert failure
    assert!(matches!(
        out,
        Err(AdapterCompileError::Script {
            source: ScriptAdapterCompileError::InvalidCommand { .. }
        })
    ));
}

#[tokio::test]
async fn failed_command_not_found() {
    // Define adapter
    let v = ScriptAdapter {
        command: CommandField::Command("invalid-command".into()),
    };

    // Invoke adapter
    let out = v.compile("/".into()).await;

    println!("{out:?}");

    // Assert failure
    assert!(matches!(
        out,
        Err(AdapterCompileError::Script {
            source: ScriptAdapterCompileError::CommandInvoke { .. }
        })
    ));
}

#[tokio::test]
async fn failed_command_error_status() {
    // Define adapter
    let v = ScriptAdapter {
        command: CommandField::Command("sh -c 'exit 1'".into()),
    };

    // Invoke adapter
    let out = v.compile("/".into()).await;

    println!("{out:?}");

    // Assert failure
    assert!(matches!(
        out,
        Err(AdapterCompileError::Script {
            source: ScriptAdapterCompileError::CommandStatus { .. }
        })
    ));
}
