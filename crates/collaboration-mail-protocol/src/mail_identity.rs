//! Mail identities reuse board validators while keeping their distinct domain meaning.
use message_board::{CanonicalHash, RecordId};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid mail {field}")]
pub struct MailValidationError {
    pub field: &'static str,
}

macro_rules! mail_identity {
    ($name:ident, $inner:ty, $field:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name($inner);
        impl TryFrom<String> for $name {
            type Error = MailValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                <$inner>::try_from(value)
                    .map(Self)
                    .map_err(|_| MailValidationError { field: $field })
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.into()
            }
        }
        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                <$inner>::json_schema(generator)
            }
        }
    };
}
mail_identity!(MailItemId, RecordId, "itemId");
mail_identity!(MembershipId, RecordId, "membership");
mail_identity!(MailDigest, CanonicalHash, "digest");

impl MailItemId {
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn generate() -> Self {
        Self(RecordId::generate())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MessageText(String);
impl TryFrom<String> for MessageText {
    type Error = MailValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 65_536
            || value
                .chars()
                .any(|character| character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
        {
            return Err(MailValidationError { field: "text" });
        }
        Ok(Self(value))
    }
}
impl From<MessageText> for String {
    fn from(value: MessageText) -> Self {
        value.0
    }
}
impl MessageText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for MessageText {
    fn schema_name() -> Cow<'static, str> {
        "MessageText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"maxLength":65536,"x-maxUtf8Bytes":65536,"pattern":"^[^\\u0000-\\u0008\\u000B\\u000C\\u000E-\\u001F]+$"})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Excerpt(String);
impl TryFrom<String> for Excerpt {
    type Error = MailValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.chars().count() > 280 {
            return Err(MailValidationError { field: "excerpt" });
        }
        Ok(Self(value))
    }
}
impl From<Excerpt> for String {
    fn from(value: Excerpt) -> Self {
        value.0
    }
}
impl Excerpt {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for Excerpt {
    fn schema_name() -> Cow<'static, str> {
        "Excerpt".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","maxLength":280})
    }
}

macro_rules! mail_counter {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(u64);
        impl $name {
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}
mail_counter!(StatusVersion);
mail_counter!(ReportSeq);
mail_counter!(MailSeq);
