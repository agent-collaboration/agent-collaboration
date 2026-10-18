//! Variable-length ordered keys allow a checklist move to rewrite only its own row.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::TextValidationError;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SubtaskPlace(Vec<u8>);

impl TryFrom<String> for SubtaskPlace {
    type Error = TextValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let invalid = || TextValidationError {
            field: "place",
            requirement: "must be an even lowercase hex key ending in a nonzero byte",
        };
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
        schemars::json_schema!({"type":"string","minLength":2,
            "pattern":"^(?:[0-9a-f]{2})*(?:[0-9a-f][1-9a-f]|[1-9a-f]0)$"})
    }
}

impl SubtaskPlace {
    pub fn between(
        lower: Option<&Self>,
        upper: Option<&Self>,
    ) -> Result<Self, TextValidationError> {
        if matches!((lower, upper), (Some(lower), Some(upper)) if lower >= upper) {
            return Err(TextValidationError {
                field: "place",
                requirement: "lower bound must precede upper bound",
            });
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
                let middle =
                    u8::try_from(low + (high - low) / 2).map_err(|_| TextValidationError {
                        field: "place",
                        requirement: "generated key byte is out of range",
                    })?;
                result.push(middle);
                return Ok(Self(result));
            }
            result.push(u8::try_from(low).map_err(|_| TextValidationError {
                field: "place",
                requirement: "lower key byte is out of range",
            })?);
            if high > low {
                upper_bytes = None;
            }
            index += 1;
        }
    }
}
