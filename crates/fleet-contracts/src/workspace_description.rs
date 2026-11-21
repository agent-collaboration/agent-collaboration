use serde::Deserialize;

/// The UTF-8 byte limit for signed workspace descriptions and stored directory-row decoding.
pub const MAXIMUM_DESCRIPTION_BYTES: usize = 2048;

pub fn description_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type":"string",
        "maxLength":MAXIMUM_DESCRIPTION_BYTES,
        "x-maxUtf8Bytes":MAXIMUM_DESCRIPTION_BYTES
    })
}

pub fn deserialize_description<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    let description = String::deserialize(deserializer)?;
    if description.len() > MAXIMUM_DESCRIPTION_BYTES {
        return Err(serde::de::Error::custom(
            "description exceeds 2048 UTF-8 bytes",
        ));
    }
    T::try_from(description).map_err(serde::de::Error::custom)
}
