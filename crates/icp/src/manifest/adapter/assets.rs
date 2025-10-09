use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DirField {
    /// Directory used to synchronize an assets canister
    Dir(String),

    /// Set of directories used to synchronize an assets canister
    Dirs(Vec<String>),
}

impl DirField {
    pub fn as_vec(&self) -> Vec<String> {
        match self {
            Self::Dir(dir) => vec![dir.clone()],
            Self::Dirs(dirs) => dirs.clone(),
        }
    }
}

/// Configuration for a custom canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Adapter {
    /// Directory used to synchronize an assets canister
    #[serde(flatten)]
    pub dir: DirField,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dir = match &self.dir {
            DirField::Dir(d) => format!("directory: {d}"),
            DirField::Dirs(ds) => format!("{} directories", ds.len()),
        };

        write!(f, "({dir})")
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Error, Ok};

    use super::*;

    #[test]
    fn dir() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                dir: dist
                "#
            )?,
            Adapter {
                dir: DirField::Dir("dist".to_string())
            },
        );

        Ok(())
    }

    #[test]
    fn dirs() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Adapter>(
                r#"
                dirs:
                  - dir-1
                  - dir-2
                  - dir-3
                "#
            )?,
            Adapter {
                dir: DirField::Dirs(vec![
                    "dir-1".to_string(),
                    "dir-2".to_string(),
                    "dir-3".to_string(),
                ])
            },
        );

        Ok(())
    }
}
