//! Checks on what an extension declares, made at install so that nothing it
//! declared can make icp-cli's own command tree fail to build.

use std::collections::BTreeSet;

use snafu::prelude::*;

use crate::{ArgKind, Info};

/// Long flags icp-cli owns: its global flags and the standard ones it attaches.
pub const RESERVED_LONGS: &[&str] = &[
    "help",
    "version",
    "debug",
    "project-root-override",
    "identity-password-file",
    "identity",
    "environment",
    "network",
    "root-key",
];

/// Short flags icp-cli owns.
pub const RESERVED_SHORTS: &[char] = &['h', 'V', 'e', 'n', 'k'];

/// Argument names that clash with the ids of icp-cli's own arguments.
const RESERVED_NAMES: &[&str] = &[
    "help",
    "version",
    "debug",
    "project_root_override",
    "identity_password_file",
    "identity",
    "environment",
    "network",
    "root_key",
    // The ids of the groups the standard network flags belong to.
    "environment-select",
    "network-select",
];

#[derive(Debug, Snafu)]
pub enum InvalidInfo {
    #[snafu(display(
        "'{name}' is not a valid extension name: use lowercase letters, digits and '-', \
         starting with a letter"
    ))]
    Name { name: String },

    #[snafu(display("extension version '{version}' is not valid semver"))]
    Version {
        version: String,
        source: semver::Error,
    },

    #[snafu(display(
        "the extension declares the management canister, which extensions may not call"
    ))]
    ManagementCanister,

    #[snafu(display(
        "the {field} contains a control or bidirectional-formatting character, \
         which could rewrite what the terminal shows"
    ))]
    UnprintableText { field: String },

    #[snafu(display("'{segment}' in command '{path}' is not a valid command name"))]
    CommandName { path: String, segment: String },

    #[snafu(display("command '{path}' is declared more than once"))]
    DuplicateCommand { path: String },

    #[snafu(display("argument '{arg}' of command '{path}' is not a valid argument name"))]
    ArgName { path: String, arg: String },

    #[snafu(display("argument '{arg}' of command '{path}' is declared more than once"))]
    DuplicateArg { path: String, arg: String },

    #[snafu(display("argument name '{arg}' of command '{path}' is reserved by icp-cli"))]
    ReservedArgName { path: String, arg: String },

    #[snafu(display("'--{long}' of command '{path}' is reserved by icp-cli"))]
    ReservedLong { path: String, long: String },

    #[snafu(display("'-{short}' of command '{path}' is reserved by icp-cli"))]
    ReservedShort { path: String, short: char },

    #[snafu(display("'{flag}' is used by more than one argument of command '{path}'"))]
    DuplicateFlag { path: String, flag: String },

    #[snafu(display(
        "argument '{arg}' of command '{path}' is a flag, which can be neither positional, \
         required, nor have a default"
    ))]
    FlagShape { path: String, arg: String },

    #[snafu(display(
        "positional argument '{arg}' of command '{path}' takes many values, so it must come last"
    ))]
    PositionalNotLast { path: String, arg: String },

    #[snafu(display(
        "the default '{default}' of argument '{arg}' of command '{path}' is not one of its \
         possible values"
    ))]
    DefaultNotPossible {
        path: String,
        arg: String,
        default: String,
    },

    #[snafu(display(
        "required positional argument '{arg}' of command '{path}' comes after an optional one"
    ))]
    RequiredAfterOptional { path: String, arg: String },

    #[snafu(display(
        "command '{path}' has subcommands, so it is a group: it can take no arguments or \
         standard flags of its own"
    ))]
    GroupWithArgs { path: String },
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An argument name or long flag: no leading `-`, which clap would strip,
/// letting `-debug` past the reserved and duplicate checks as `--debug`.
fn is_arg_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A character that could make the terminal show something other than the
/// text: C0/C1 controls (escape sequences, carriage return, backspace…) and
/// the Unicode bidirectional overrides, embeddings, isolates and marks.
fn is_unprintable(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Refuse free text the extension chose that `icp` prints: a label or summary
/// carrying escape sequences could erase lines of the install prompt, a ledger
/// and its warning among them. `multiline` lets `\n` through, for help text.
fn ensure_printable(
    text: &str,
    multiline: bool,
    field: impl FnOnce() -> String,
) -> Result<(), InvalidInfo> {
    ensure!(
        !text
            .chars()
            .any(|c| is_unprintable(c) && !(multiline && c == '\n')),
        UnprintableTextSnafu { field: field() }
    );
    Ok(())
}

/// `text` with every character `validate` would refuse in it escaped, for
/// printing what an extension declared before `validate` checked for them.
pub fn escape_unprintable(text: &str) -> String {
    text.chars()
        .map(|c| match is_unprintable(c) {
            true => c.escape_unicode().to_string(),
            false => c.to_string(),
        })
        .collect()
}

/// Check what an extension declared. Whether its name collides with a
/// built-in command is for the caller to check, which knows the built-ins.
pub fn validate(info: &Info) -> Result<(), InvalidInfo> {
    ensure!(
        is_name(&info.name) && info.name != "help",
        NameSnafu { name: &info.name }
    );
    semver::Version::parse(&info.version).context(VersionSnafu {
        version: &info.version,
    })?;
    ensure!(
        !info
            .canisters
            .iter()
            .any(|c| c.id == candid::Principal::management_canister()),
        ManagementCanisterSnafu
    );
    ensure_printable(&info.summary, false, || "summary".into())?;
    for canister in &info.canisters {
        ensure_printable(&canister.label, false, || {
            format!("label of canister {}", canister.id)
        })?;
    }

    let mut paths = BTreeSet::new();
    for command in &info.commands {
        let path = std::iter::once(info.name.as_str())
            .chain(command.path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        for segment in &command.path {
            ensure!(
                is_name(segment) && segment != "help",
                CommandNameSnafu {
                    path: &path,
                    segment
                }
            );
        }
        ensure!(
            paths.insert(&command.path),
            DuplicateCommandSnafu { path: &path }
        );
        ensure_printable(&command.about, true, || {
            format!("description of command '{path}'")
        })?;
        if let Some(network) = &command.default_network {
            ensure_printable(network, false, || {
                format!("default network of command '{path}'")
            })?;
        }

        let mut names = BTreeSet::new();
        let mut flags = BTreeSet::new();
        let positionals: Vec<_> = command.args.iter().filter(|a| a.is_positional()).collect();
        for arg in &command.args {
            let name = &arg.name;
            ensure!(
                is_arg_name(name),
                ArgNameSnafu {
                    path: &path,
                    arg: name
                }
            );
            ensure!(
                !RESERVED_NAMES.contains(&name.as_str()),
                ReservedArgNameSnafu {
                    path: &path,
                    arg: name
                }
            );
            ensure!(
                names.insert(name),
                DuplicateArgSnafu {
                    path: &path,
                    arg: name
                }
            );
            let field = |what: &str| format!("{what} of argument '{name}' of command '{path}'");
            ensure_printable(&arg.help, true, || field("help"))?;
            for value in arg
                .value_name
                .iter()
                .chain(&arg.default_value)
                .chain(&arg.possible_values)
            {
                ensure_printable(value, false, || field("value name or possible value"))?;
            }
            if let Some(long) = &arg.long {
                ensure!(
                    is_arg_name(long),
                    ArgNameSnafu {
                        path: &path,
                        arg: long
                    }
                );
                ensure!(
                    !RESERVED_LONGS.contains(&long.as_str()),
                    ReservedLongSnafu { path: &path, long }
                );
                ensure!(
                    flags.insert(format!("--{long}")),
                    DuplicateFlagSnafu {
                        path: &path,
                        flag: format!("--{long}")
                    }
                );
            }
            if let Some(short) = arg.short {
                ensure!(
                    short.is_ascii_alphanumeric(),
                    ArgNameSnafu {
                        path: &path,
                        arg: short.to_string()
                    }
                );
                ensure!(
                    !RESERVED_SHORTS.contains(&short),
                    ReservedShortSnafu { path: &path, short }
                );
                ensure!(
                    flags.insert(format!("-{short}")),
                    DuplicateFlagSnafu {
                        path: &path,
                        flag: format!("-{short}")
                    }
                );
            }
            if let Some(default) = &arg.default_value {
                ensure!(
                    arg.possible_values.is_empty() || arg.possible_values.contains(default),
                    DefaultNotPossibleSnafu {
                        path: &path,
                        arg: name,
                        default
                    }
                );
            }
            if arg.kind == ArgKind::Flag {
                ensure!(
                    !arg.is_positional() && !arg.required && arg.default_value.is_none(),
                    FlagShapeSnafu {
                        path: &path,
                        arg: name
                    }
                );
            }
        }
        // clap takes positionals in order, so an optional one cannot come
        // before one that is required.
        if let Some(arg) = positionals
            .iter()
            .skip_while(|a| a.required)
            .find(|a| a.required)
        {
            return RequiredAfterOptionalSnafu {
                path: &path,
                arg: &arg.name,
            }
            .fail();
        }
        if let Some((_, init)) = positionals.split_last()
            && let Some(arg) = init.iter().find(|a| a.kind == ArgKind::Values)
        {
            return PositionalNotLastSnafu {
                path: &path,
                arg: &arg.name,
            }
            .fail();
        }
        let is_group = info
            .commands
            .iter()
            .any(|c| c.path.len() > command.path.len() && c.path.starts_with(&command.path));
        ensure!(
            !is_group || (command.args.is_empty() && command.standard.is_empty()),
            GroupWithArgsSnafu { path: &path }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arg, Command};

    fn info(commands: Vec<Command>) -> Info {
        Info {
            name: "demo".into(),
            version: "0.1.0".into(),
            summary: "A demo".into(),
            commands,
            canisters: vec![],
        }
    }

    fn command(path: &[&str], args: Vec<Arg>) -> Command {
        Command {
            path: path.iter().map(|s| s.to_string()).collect(),
            about: String::new(),
            args,
            standard: vec![],
            default_network: None,
        }
    }

    fn option(name: &str, long: Option<&str>, short: Option<char>, kind: ArgKind) -> Arg {
        Arg {
            name: name.into(),
            long: long.map(Into::into),
            short,
            kind,
            required: false,
            default_value: None,
            possible_values: vec![],
            value_name: None,
            help: String::new(),
        }
    }

    #[test]
    fn a_well_formed_extension_passes() {
        let info = info(vec![
            command(
                &["cycles", "buy"],
                vec![option("amount", Some("amount"), Some('a'), ArgKind::Value)],
            ),
            command(
                &["domain", "list"],
                vec![option("json", Some("json"), None, ArgKind::Flag)],
            ),
            command(
                &["domain", "update"],
                vec![option("name", None, None, ArgKind::Value)],
            ),
        ]);
        validate(&info).unwrap();
    }

    #[test]
    fn a_reserved_flag_is_refused() {
        let info = info(vec![command(
            &["x"],
            vec![option("env", Some("environment"), None, ArgKind::Value)],
        )]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::ReservedLong { .. })
        ));

        let info = self::info(vec![command(
            &["x"],
            vec![option("env", None, Some('e'), ArgKind::Value)],
        )]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::ReservedShort { .. })
        ));
    }

    #[test]
    fn duplicate_commands_and_flags_are_refused() {
        let info = info(vec![command(&["x"], vec![]), command(&["x"], vec![])]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::DuplicateCommand { .. })
        ));

        let info = self::info(vec![command(
            &["x"],
            vec![
                option("a", Some("same"), None, ArgKind::Value),
                option("b", Some("same"), None, ArgKind::Value),
            ],
        )]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::DuplicateFlag { .. })
        ));
    }

    #[test]
    fn a_network_group_id_is_reserved() {
        for name in ["network-select", "environment-select"] {
            let info = info(vec![command(
                &["x"],
                vec![option(name, None, None, ArgKind::Value)],
            )]);
            assert!(matches!(
                validate(&info),
                Err(InvalidInfo::ReservedArgName { .. })
            ));
        }
    }

    #[test]
    fn a_leading_hyphen_is_refused() {
        // `-debug` would become `--debug`, past the reserved check.
        let info = info(vec![command(
            &["x"],
            vec![option("d", Some("-debug"), None, ArgKind::Flag)],
        )]);
        assert!(matches!(validate(&info), Err(InvalidInfo::ArgName { .. })));
        let info = self::info(vec![command(
            &["x"],
            vec![option("-d", None, None, ArgKind::Value)],
        )]);
        assert!(matches!(validate(&info), Err(InvalidInfo::ArgName { .. })));
    }

    #[test]
    fn a_default_must_be_a_possible_value() {
        let mut env = option("env", Some("env"), None, ArgKind::Value);
        env.possible_values = vec!["local".into(), "ic".into()];
        env.default_value = Some("prod".into());
        let info = info(vec![command(&["x"], vec![env.clone()])]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::DefaultNotPossible { .. })
        ));

        env.default_value = Some("ic".into());
        validate(&self::info(vec![command(&["x"], vec![env])])).unwrap();
    }

    #[test]
    fn a_required_positional_cannot_follow_an_optional_one() {
        let source = option("source", None, None, ArgKind::Value);
        let mut destination = option("destination", None, None, ArgKind::Value);
        destination.required = true;
        let info = info(vec![command(
            &["x"],
            vec![source.clone(), destination.clone()],
        )]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::RequiredAfterOptional { .. })
        ));

        // The other way round is how clap reads them.
        validate(&self::info(vec![command(
            &["x"],
            vec![destination, source],
        )]))
        .unwrap();
    }

    #[test]
    fn a_group_takes_no_arguments() {
        let group = command(
            &["cycles"],
            vec![option("x", Some("x"), None, ArgKind::Flag)],
        );
        let info = info(vec![group, command(&["cycles", "buy"], vec![])]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::GroupWithArgs { .. })
        ));

        // Declared with no arguments, a group only names itself.
        let info = self::info(vec![
            command(&["cycles"], vec![]),
            command(&["cycles", "buy"], vec![]),
        ]);
        validate(&info).unwrap();
    }

    #[test]
    fn the_management_canister_cannot_be_declared() {
        let mut bad = info(vec![]);
        bad.canisters = vec![crate::AllowedCanister {
            id: candid::Principal::management_canister(),
            label: "management".into(),
        }];
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::ManagementCanister)
        ));
    }

    #[test]
    fn text_that_could_rewrite_the_terminal_is_refused() {
        // Erase the line, move up, erase that one too: hides the line above.
        let hide = "rate\x1b[2K\x1b[1A\x1b[2K\r";
        let canister = |label: &str| crate::AllowedCanister {
            id: candid::Principal::anonymous(),
            label: label.into(),
        };

        let mut bad = info(vec![]);
        bad.canisters = vec![canister("ledger"), canister(hide)];
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::UnprintableText { .. })
        ));

        let mut bad = info(vec![]);
        bad.canisters = vec![canister("ICP \u{202E}regdel")];
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::UnprintableText { .. })
        ));

        let mut bad = info(vec![]);
        bad.summary = hide.into();
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::UnprintableText { .. })
        ));

        let mut arg = option("x", Some("x"), None, ArgKind::Value);
        arg.help = hide.into();
        let bad = info(vec![command(&["x"], vec![arg.clone()])]);
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::UnprintableText { .. })
        ));

        let mut bad = info(vec![command(&["x"], vec![])]);
        bad.commands[0].about = "fine\nthen\rnot".into();
        assert!(matches!(
            validate(&bad),
            Err(InvalidInfo::UnprintableText { .. })
        ));

        // A line break in help text is fine; in a label it is not.
        arg.help = "First line.\nSecond line, with ünïcödé.".into();
        let mut good = info(vec![command(&["x"], vec![arg])]);
        good.canisters = vec![canister("Exchange rate canister")];
        validate(&good).unwrap();
        good.canisters = vec![canister("two\nlines")];
        assert!(matches!(
            validate(&good),
            Err(InvalidInfo::UnprintableText { .. })
        ));
    }

    #[test]
    fn unprintable_text_is_escaped_for_display() {
        assert_eq!(
            escape_unprintable("rate\x1b[2K\r \u{202E}x ünï"),
            "rate\\u{1b}[2K\\u{d} \\u{202e}x ünï"
        );
    }

    #[test]
    fn a_bad_name_is_refused() {
        let mut bad = info(vec![]);
        bad.name = "Cycle Pay".into();
        assert!(matches!(validate(&bad), Err(InvalidInfo::Name { .. })));
    }

    #[test]
    fn a_many_valued_positional_must_be_last() {
        let info = info(vec![command(
            &["x"],
            vec![
                option("many", None, None, ArgKind::Values),
                option("one", None, None, ArgKind::Value),
            ],
        )]);
        assert!(matches!(
            validate(&info),
            Err(InvalidInfo::PositionalNotLast { .. })
        ));
    }
}
