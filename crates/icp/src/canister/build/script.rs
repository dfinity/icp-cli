use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CommandField {
    /// Command used to build a canister
    Command(String),

    /// Set of commands used to build a canister
    Commands(Vec<String>),
}

impl CommandField {
    fn _as_vec(&self) -> Vec<String> {
        match self {
            Self::Command(cmd) => vec![cmd.clone()],
            Self::Commands(cmds) => cmds.clone(),
        }
    }
}

/// Configuration for a custom canister build adapter.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct Adapter {
    /// Command used to build a canister
    #[serde(flatten)]
    pub command: CommandField,

    #[serde(skip)]
    pub stdio_sender: Option<Sender<String>>,
}

impl PartialEq for Adapter {
    fn eq(&self, other: &Self) -> bool {
        self.command == other.command
    }
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cmd = match &self.command {
            CommandField::Command(c) => format!("command: {c}"),
            CommandField::Commands(cs) => format!("{} commands", cs.len()),
        };

        write!(f, "({cmd})")
    }
}

impl Adapter {
    pub fn with_stdio_sender(&self, sender: Sender<String>) -> Self {
        let mut v = self.clone();
        v.stdio_sender = Some(sender);
        v
    }
}
