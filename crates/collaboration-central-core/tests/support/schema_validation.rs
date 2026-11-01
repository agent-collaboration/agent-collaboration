use jsonschema::{Keyword, ValidationError, Validator};
use serde_json::Value;

struct Utf8ByteLimit {
    maximum_bytes: u64,
}

impl<'instance> Keyword<'instance> for Utf8ByteLimit {
    fn validate(&self, instance: &'instance Value) -> Result<(), ValidationError<'instance>> {
        if self.is_valid(instance) {
            Ok(())
        } else {
            Err(ValidationError::custom("text exceeds its UTF-8 byte limit"))
        }
    }

    fn is_valid(&self, instance: &'instance Value) -> bool {
        instance.as_str().is_none_or(|text| {
            u64::try_from(text.len()).is_ok_and(|length| length <= self.maximum_bytes)
        })
    }
}

pub fn contract_validator(schema: &Value) -> Result<Validator, ValidationError<'static>> {
    jsonschema::options()
        .with_keyword("x-maxUtf8Bytes", |_, value, _| {
            let maximum_bytes = value.as_u64().ok_or_else(|| {
                ValidationError::custom("x-maxUtf8Bytes must be a nonnegative integer")
            })?;
            Ok(Box::new(Utf8ByteLimit { maximum_bytes }))
        })
        .build(schema)
}
