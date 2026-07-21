/// Deserialization helpers
use serde::Deserialize;
use serde::de::{self, Deserializer};

/// Requires that a vector has at least one entry
pub(crate) fn non_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let v = Vec::<T>::deserialize(deserializer)?;
    if v.is_empty() {
        return Err(de::Error::custom("Array must not be empty"));
    }
    Ok(v)
}
