use std::fmt;

use candid::Principal;
use ic_management_canister_types::{LogVisibility, StatusVisibility};
use serde::{Deserialize, Serialize, Serializer, de};

/// Who may read a visibility-gated part of a canister.
///
/// The management canister exposes several structurally identical visibility
/// settings, each with its own Candid type. This is the single form the CLI
/// parses, compares, and renders; conversions to and from the Candid types are
/// generated below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Only the canister's controllers.
    Controllers,
    /// Anyone.
    Public,
    /// The canister's controllers plus the listed principals.
    AllowedViewers(Vec<Principal>),
}

impl Visibility {
    /// Deserializes the manifest form: either `controllers` / `public`, or a
    /// `{ allowed_viewers: [...] }` mapping. `setting` names the manifest field
    /// so a bad value points at the setting that carried it.
    fn deserialize_setting<'de, D>(deserializer: D, setting: &'static str) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct VisibilityVisitor(&'static str);

        impl<'de> de::Visitor<'de> for VisibilityVisitor {
            type Value = Visibility;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("'controllers', 'public', or object with 'allowed_viewers'")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "controllers" => Ok(Visibility::Controllers),
                    "public" => Ok(Visibility::Public),
                    _ => Err(E::custom(format!(
                        "unknown {} value: '{}', expected 'controllers' or 'public'",
                        self.0, value
                    ))),
                }
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut allowed_viewers: Option<Vec<Principal>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "allowed_viewers" => {
                            if allowed_viewers.is_some() {
                                return Err(de::Error::duplicate_field("allowed_viewers"));
                            }
                            allowed_viewers = Some(map.next_value()?);
                        }
                        _ => return Err(de::Error::unknown_field(&key, &["allowed_viewers"])),
                    }
                }

                allowed_viewers
                    .map(Visibility::AllowedViewers)
                    .ok_or_else(|| de::Error::missing_field("allowed_viewers"))
            }
        }

        deserializer.deserialize_any(VisibilityVisitor(setting))
    }
}

impl Serialize for Visibility {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Visibility::Controllers => serializer.serialize_str("controllers"),
            Visibility::Public => serializer.serialize_str("public"),
            Visibility::AllowedViewers(allowed_viewers) => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("allowed_viewers", allowed_viewers)?;
                map.end()
            }
        }
    }
}

macro_rules! candid_conversions {
    ($candid:ident) => {
        impl From<Visibility> for $candid {
            fn from(value: Visibility) -> Self {
                match value {
                    Visibility::Controllers => Self::Controllers,
                    Visibility::Public => Self::Public,
                    Visibility::AllowedViewers(viewers) => Self::AllowedViewers(viewers),
                }
            }
        }

        impl From<$candid> for Visibility {
            fn from(value: $candid) -> Self {
                match value {
                    $candid::Controllers => Self::Controllers,
                    $candid::Public => Self::Public,
                    $candid::AllowedViewers(viewers) => Self::AllowedViewers(viewers),
                }
            }
        }
    };
}

candid_conversions!(LogVisibility);
candid_conversions!(StatusVisibility);

fn visibility_schema(description: &str, subject: &str) -> schemars::Schema {
    schemars::json_schema!({
        "description": description,
        "oneOf": [
            {
                "type": "string",
                "enum": ["controllers", "public"],
                "description": format!("'controllers' (only the canister's controllers can {subject}) or 'public' (anyone can)"),
            },
            {
                "type": "object",
                "properties": {
                    "allowed_viewers": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "A principal ID",
                        },
                        "description": format!("Principal IDs that can {subject}, in addition to the controllers"),
                    }
                },
                "required": ["allowed_viewers"],
                "additionalProperties": false,
                "description": format!("Specific principals that can {subject}"),
            }
        ]
    })
}

/// Declares a manifest-level visibility setting: a [`Visibility`] newtype that
/// names itself in parse errors and in the generated JSON schema.
macro_rules! visibility_setting {
    (
        $name:ident,
        setting = $setting:literal,
        schema = $schema:literal,
        description = $description:literal,
        subject = $subject:literal $(,)?
    ) => {
        #[doc = $description]
        #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub Visibility);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Visibility::deserialize_setting(deserializer, $setting).map(Self)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed($schema)
            }

            fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                visibility_schema($description, $subject)
            }
        }

        impl From<$name> for Visibility {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

visibility_setting!(
    LogVisibilityDef,
    setting = "log_visibility",
    schema = "LogVisibility",
    description = "Controls who can read canister logs.",
    subject = "read the logs",
);

visibility_setting!(
    StatusVisibilityDef,
    setting = "status_visibility",
    schema = "StatusVisibility",
    description = "Controls who can read the canister's status.",
    subject = "read the status",
);

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    #[test]
    fn deserialize_controllers() {
        let parsed: LogVisibilityDef = serde_yaml::from_str("controllers").unwrap();
        assert_eq!(parsed.0, Visibility::Controllers);
    }

    #[test]
    fn deserialize_public() {
        let parsed: LogVisibilityDef = serde_yaml::from_str("public").unwrap();
        assert_eq!(parsed.0, Visibility::Public);
    }

    #[test]
    fn deserialize_allowed_viewers() {
        let yaml = r#"
allowed_viewers:
  - "aaaaa-aa"
  - "2vxsx-fae"
"#;
        let parsed: LogVisibilityDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            parsed.0,
            Visibility::AllowedViewers(vec![principal("aaaaa-aa"), principal("2vxsx-fae")])
        );
    }

    #[test]
    fn deserialize_allowed_viewers_empty() {
        let parsed: LogVisibilityDef = serde_yaml::from_str("allowed_viewers: []").unwrap();
        assert_eq!(parsed.0, Visibility::AllowedViewers(vec![]));
    }

    #[test]
    fn deserialize_invalid_string_names_the_setting() {
        let err = serde_yaml::from_str::<LogVisibilityDef>("invalid")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown log_visibility value"), "{err}");

        let err = serde_yaml::from_str::<StatusVisibilityDef>("invalid")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown status_visibility value"), "{err}");
    }

    #[test]
    fn deserialize_invalid_field() {
        let err = serde_yaml::from_str::<StatusVisibilityDef>("unknown_field: []")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn serialize_round_trips_the_manifest_form() {
        for (value, expected) in [
            (Visibility::Controllers, "controllers"),
            (Visibility::Public, "public"),
        ] {
            let yaml = serde_yaml::to_string(&StatusVisibilityDef(value.clone())).unwrap();
            assert_eq!(yaml.trim(), expected);
            assert_eq!(
                serde_yaml::from_str::<StatusVisibilityDef>(&yaml)
                    .unwrap()
                    .0,
                value
            );
        }

        let viewers =
            Visibility::AllowedViewers(vec![principal("aaaaa-aa"), principal("2vxsx-fae")]);
        let yaml = serde_yaml::to_string(&StatusVisibilityDef(viewers.clone())).unwrap();
        assert_eq!(
            serde_yaml::from_str::<StatusVisibilityDef>(&yaml)
                .unwrap()
                .0,
            viewers
        );
    }

    #[test]
    fn converts_to_and_from_candid_types() {
        let cases = [
            Visibility::Controllers,
            Visibility::Public,
            Visibility::AllowedViewers(vec![principal("aaaaa-aa")]),
        ];

        for value in cases {
            let log: LogVisibility = value.clone().into();
            assert_eq!(Visibility::from(log), value);

            let status: StatusVisibility = value.clone().into();
            assert_eq!(Visibility::from(status), value);
        }
    }

    #[test]
    fn schema_names_are_distinct() {
        use schemars::JsonSchema;
        assert_eq!(LogVisibilityDef::schema_name(), "LogVisibility");
        assert_eq!(StatusVisibilityDef::schema_name(), "StatusVisibility");
    }
}
