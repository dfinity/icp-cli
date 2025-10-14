use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
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
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
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
    use anyhow::Error;

    use super::*;

    #[test]
    fn command() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                command: echo hi
                "#
            )?,
            Adapter {
                command: CommandField::Command("echo hi".to_string()),
            },
        );

        Ok(())
    }

    #[test]
    fn command_multiline() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                command: |
                  echo hi
                "#
            )?,
            Adapter {
                command: CommandField::Command("echo hi\n".to_string()),
            },
        );

        Ok(())
    }

    #[test]
    fn commands() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                commands:
                  - echo hi
                  - echo bye
                "#
            )?,
            Adapter {
                command: CommandField::Commands(vec![
                    "echo hi".to_string(),
                    "echo bye".to_string(),
                ]),
            },
        );

        Ok(())
    }
}
