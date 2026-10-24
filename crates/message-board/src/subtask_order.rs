//! Variable-length ordered keys allow a checklist move to rewrite only its own row.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

pub const MAX_ORDER_KEY_HEX_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SubtaskPlaceError {
    #[error(
        "place must be an even lowercase hex key ending in a nonzero byte, within ordered bounds"
    )]
    InvalidShape,
    #[error("orderKeyTooLong: {length} hex bytes exceeds {limit}")]
    OrderKeyTooLong { length: usize, limit: usize },
}

fn validate_key_length(length: usize) -> Result<(), SubtaskPlaceError> {
    if length > MAX_ORDER_KEY_HEX_BYTES {
        return Err(SubtaskPlaceError::OrderKeyTooLong {
            length,
            limit: MAX_ORDER_KEY_HEX_BYTES,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SubtaskPlace(Vec<u8>);

impl TryFrom<String> for SubtaskPlace {
    type Error = SubtaskPlaceError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_key_length(value.len())?;
        let invalid = || SubtaskPlaceError::InvalidShape;
        if value.is_empty() || !value.len().is_multiple_of(2) {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        for &[high, low] in value.as_bytes().as_chunks::<2>().0 {
            let high = decode_hex(high).ok_or_else(invalid)?;
            let low = decode_hex(low).ok_or_else(invalid)?;
            bytes.push(high * 16 + low);
        }
        if bytes.last() == Some(&0) {
            return Err(invalid());
        }
        Ok(Self(bytes))
    }
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

impl From<SubtaskPlace> for String {
    fn from(value: SubtaskPlace) -> Self {
        value.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl JsonSchema for SubtaskPlace {
    fn schema_name() -> Cow<'static, str> {
        "SubtaskPlace".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({"type":"string","minLength":2,"maxLength":8192,
            "pattern":"^(?:[0-9a-f]{2})*(?:[0-9a-f][1-9a-f]|[1-9a-f]0)$"})
    }
}

impl SubtaskPlace {
    pub fn between(lower: Option<&Self>, upper: Option<&Self>) -> Result<Self, SubtaskPlaceError> {
        if matches!((lower, upper), (Some(lower), Some(upper)) if lower >= upper) {
            return Err(SubtaskPlaceError::InvalidShape);
        }
        let mut result = Vec::new();
        let mut index = 0;
        let mut upper_bytes = upper.map(|place| place.0.as_slice());
        loop {
            let low = lower
                .and_then(|place| place.0.get(index))
                .copied()
                .map_or(0, u16::from);
            let high = upper_bytes
                .and_then(|bytes| bytes.get(index))
                .copied()
                .map_or(256, u16::from);
            if high > low + 1 {
                let middle = u8::try_from(low + (high - low) / 2)
                    .map_err(|_| SubtaskPlaceError::InvalidShape)?;
                result.push(middle);
                validate_key_length(result.len() * 2)?;
                return Ok(Self(result));
            }
            result.push(u8::try_from(low).map_err(|_| SubtaskPlaceError::InvalidShape)?);
            validate_key_length((result.len() + 1) * 2)?;
            if high > low {
                upper_bytes = None;
            }
            index += 1;
        }
    }
}
