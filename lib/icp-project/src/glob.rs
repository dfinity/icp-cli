use std::fmt;

use glob::{Pattern as GlobPattern, PatternError};
use serde::{
    Deserialize, Deserializer,
    de::{self, Visitor},
};

#[derive(Debug)]
pub struct Pattern(GlobPattern);

impl Pattern {
    pub fn new(pattern: &str) -> Result<Pattern, PatternError> {
        GlobPattern::new(pattern).map(Pattern)
    }

    pub fn value(&self) -> GlobPattern {
        self.0.clone()
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PatternVisitor;

        impl Visitor<'_> for PatternVisitor {
            type Value = Pattern;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid glob pattern string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Pattern, E>
            where
                E: de::Error,
            {
                GlobPattern::new(value)
                    .map(Pattern)
                    .map_err(|e: PatternError| E::custom(format!("invalid glob pattern: {e}")))
            }
        }

        deserializer.deserialize_str(PatternVisitor)
    }
}
