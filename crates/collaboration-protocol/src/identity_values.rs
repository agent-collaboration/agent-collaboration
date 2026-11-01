//! Validated scalar values shared by endpoint metadata and other contracts.
use serde::{Deserialize, Serialize};

/// Bounded public text, distinct from any native session identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NonEmptyText(String);
impl TryFrom<String> for NonEmptyText {
    type Error = crate::IdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::limits::LimitName::TextMaxBytes.validate(value.len() as u64)?;
        if value.contains('\0') {
            return Err(crate::IdentityError::Invalid("NUL in text"));
        }
        Ok(Self(value))
    }
}
impl From<NonEmptyText> for String {
    fn from(value: NonEmptyText) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SchemaDigest(String);
impl TryFrom<String> for SchemaDigest {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let digits = value
            .strip_prefix("sha256:")
            .ok_or("invalid schema digest")?;
        if digits.len() != 64
            || !digits
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid schema digest");
        }
        Ok(Self(value))
    }
}
impl From<SchemaDigest> for String {
    fn from(value: SchemaDigest) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ObservationTimestamp(String);
impl TryFrom<String> for ObservationTimestamp {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.ends_with('Z') || chrono::DateTime::parse_from_rfc3339(&value).is_err() {
            return Err("timestamp must be RFC3339 UTC ending in Z");
        }
        Ok(Self(value))
    }
}
impl From<ObservationTimestamp> for String {
    fn from(value: ObservationTimestamp) -> Self {
        value.0
    }
}
