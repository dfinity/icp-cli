use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandField {
    /// Command used to build a canister
    Command(String),

    /// Set of commands used to build a canister
    Commands(Vec<String>),
}

impl CommandField {
    pub fn as_vec(&self) -> Vec<String> {
        match self {
            Self::Command(cmd) => vec![cmd.clone()],
            Self::Commands(cmds) => cmds.clone(),
        }
    }
}

/// Configuration for a custom canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Adapter {
    /// Command used to build a canister
    #[serde(flatten)]
    pub command: CommandField,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cmd = match &self.command {
            CommandField::Command(c) => c,
            CommandField::Commands(cs) => &cs.join("\n"),
        };

        write!(f, "{cmd}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                command: echo hi
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                command: CommandField::Command("echo hi".to_string()),
            },
        );
    }

    #[test]
    fn command_multiline() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                command: |
                  echo hi
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                command: CommandField::Command("echo hi\n".to_string()),
            },
        );
    }

    #[test]
    fn commands() {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                commands:
                  - echo hi
                  - echo bye
                "#
            )
            .expect("failed to deserialize Adapter from yaml"),
            Adapter {
                command: CommandField::Commands(vec![
                    "echo hi".to_string(),
                    "echo bye".to_string(),
                ]),
            },
        );
    }
}
