use std::io::{self, Write as _};

use clap::Args;
use clap_complete::Shell;
use icp::prelude::*;
use indoc::formatdoc;
use snafu::prelude::*;

/// Generate a shell completion script
///
/// The script is written to stdout. Save it where your shell loads completions
/// from, or source it from your shell profile.
#[derive(Debug, Args)]
#[command(after_long_help = "\
Examples:

    # Bash
    icp completions bash > ~/.local/share/bash-completion/completions/icp

    # Zsh, into a directory on your $fpath
    icp completions zsh > ~/.zfunc/_icp

    # Fish
    icp completions fish > ~/.config/fish/completions/icp.fish

    # Elvish, appended to your profile
    icp completions elvish >> ~/.elvish/rc.elv

    # PowerShell, appended to your profile
    icp completions powershell >> $PROFILE
")]
pub(crate) struct CompletionsArgs {
    /// The shell to generate a completion script for
    shell: Shell,
}

#[derive(Debug, Snafu)]
pub(crate) enum CompletionsError {
    #[snafu(display("no completion support for shell `{shell}`"))]
    UnsupportedShell { shell: Shell },

    #[snafu(display("failed to locate the running icp executable"))]
    LocateExecutable { source: io::Error },

    #[snafu(display("path to the icp executable is not valid UTF-8"))]
    ExecutablePathEncoding { source: FromPathBufError },

    #[snafu(display("failed to write the completion script to stdout"))]
    WriteScript { source: io::Error },
}

pub(crate) fn exec(args: &CompletionsArgs) -> Result<(), CompletionsError> {
    let executable = std::env::current_exe().context(LocateExecutableSnafu)?;
    let executable = PathBuf::try_from(executable).context(ExecutablePathEncodingSnafu)?;

    let script =
        loader(args.shell, &executable).context(UnsupportedShellSnafu { shell: args.shell })?;

    io::stdout()
        .write_all(script.as_bytes())
        .context(WriteScriptSnafu)
}

/// The script `icp completions` emits, for `shell` to load.
///
/// It does not contain the completion hook. It asks `executable` for the hook
/// every time the shell loads it, because the hook and `icp` talk to each other
/// over a protocol with no stability guarantee: a script that carried the hook
/// would stop completing, or complete wrongly, the moment `icp` was upgraded
/// out from under it. Fetching the hook at load time is what lets this be
/// written to a completions directory once and left alone.
///
/// `COMPLETE` is the variable [`clap_complete::CompleteEnv`] watches; the value
/// names the shell asking, and matches what `Shell` renders itself as.
fn loader(shell: Shell, executable: &Path) -> Option<String> {
    let script = match shell {
        Shell::Bash => format!("source <(COMPLETE=bash {})\n", posix_quote(executable)),

        // Autoloaded from $fpath, zsh runs this file to answer the completion
        // already in progress, while the hook only registers itself for the
        // next one; calling the hook here answers this one too, instead of the
        // first Tab coming back empty. $CURRENT is set only while completing,
        // so a profile that sources this file skips the call, and the hook must
        // be something other than this file, or a registration that failed to
        // load would recurse into it.
        Shell::Zsh => formatdoc!(
            r#"
                #compdef icp

                source <(COMPLETE=zsh {executable})

                if (( ${{CURRENT:-0}} )) && [[ -n ${{_comps[icp]}} && ${{_comps[icp]}} != ${{funcstack[1]}} ]]; then
                    ${{_comps[icp]}} "$@"
                fi
            "#,
            executable = posix_quote(executable),
        ),

        Shell::Fish => format!("COMPLETE=fish {} | source\n", fish_quote(executable)),

        Shell::Elvish => format!(
            "eval (E:COMPLETE=elvish {} | slurp)\n",
            doubling_quote(executable)
        ),

        Shell::PowerShell => format!(
            "$env:COMPLETE = 'powershell'; & {} | Out-String | Invoke-Expression; Remove-Item Env:\\COMPLETE\n",
            doubling_quote(executable)
        ),

        _ => return None,
    };
    Some(script)
}

/// Quote for bash and zsh, where `'…'` is wholly literal, so an embedded quote
/// has to be closed, escaped, and reopened.
fn posix_quote(path: &Path) -> String {
    format!("'{}'", path.as_str().replace('\'', r"'\''"))
}

/// Quote for fish, which unlike POSIX shells does honour `\` escapes inside
/// single quotes.
fn fish_quote(path: &Path) -> String {
    format!(
        "'{}'",
        path.as_str().replace('\\', r"\\").replace('\'', r"\'")
    )
}

/// Quote for elvish and PowerShell, which both escape an embedded quote by
/// doubling it.
fn doubling_quote(path: &Path) -> String {
    format!("'{}'", path.as_str().replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum as _;
    use clap_complete::env::Shells;

    use super::*;

    /// The `COMPLETE` value each loader writes has to be one `CompleteEnv`
    /// dispatches on, or the shell would call us back and get an error.
    #[test]
    fn every_supported_shell_is_one_clap_complete_answers_for() {
        let shells = Shells::builtins();

        for shell in Shell::value_variants() {
            let script = loader(*shell, Path::new("/usr/local/bin/icp"))
                .unwrap_or_else(|| panic!("no loader for `{shell}`"));
            let name = shell.to_string();

            assert!(
                shells.completer(&name).is_some(),
                "`{shell}` has a loader but `CompleteEnv` does not answer for `{name}`"
            );
            assert!(
                script.contains(&format!("COMPLETE={name}"))
                    || script.contains(&format!("COMPLETE = '{name}'")),
                "loader for `{shell}` does not ask for `{name}`: {script}"
            );
        }
    }

    #[test]
    fn quoting_closes_an_embedded_quote() {
        let path = Path::new("/o'clock/icp");

        assert_eq!(posix_quote(path), r"'/o'\''clock/icp'");
        assert_eq!(fish_quote(path), r"'/o\'clock/icp'");
        assert_eq!(doubling_quote(path), "'/o''clock/icp'");
    }
}
