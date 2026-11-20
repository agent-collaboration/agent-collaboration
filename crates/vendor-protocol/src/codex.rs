//! Codex app-server wire bindings from the pinned experimental schema and pairing.

use serde::de::{DeserializeOwned, MapAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, marker::PhantomData};

mod sealed {
    pub trait Sealed {}
}

/// A Codex client request body and its upstream-declared reply body.
pub trait CodexRequest: sealed::Sealed + Serialize + DeserializeOwned {
    const METHOD: &'static str;
    type Response: Serialize + DeserializeOwned;
}

/// A supplied string alternative in a payload that requires at least one valid alternative.
/// A different valid alternative can admit the payload even when this value is malformed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StringAlternative {
    Valid(String),
    Malformed(serde_json::Value),
}

impl StringAlternative {
    fn is_valid(&self) -> bool {
        matches!(self, Self::Valid(_))
    }
}

// Serde's derived struct decoder also accepts positional sequences. Codex's
// object schemas only admit maps. Reuse the derived field decoder on that path.
macro_rules! codex_object {
    (closed $($definition:tt)*) => {
        codex_object!(@record [#[serde(deny_unknown_fields)]] $($definition)*);
    };
    (choice [$first:ident $(, $alternative:ident)+] $(#[$documentation:meta])* $name:ident {
        $($(#[$attribute:meta])* $field:ident: $value:ty,)*
    }) => {
        $(#[$documentation])*
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name { $(pub $field: $value,)* }
        codex_object!(@decode [] [$first $(, $alternative)+]
            $name { $($(#[$attribute])* $field: $value,)* });
        impl Serialize for $name {
            fn serialize<Encoder: serde::Serializer>(
                &self, encoder: Encoder,
            ) -> Result<Encoder::Ok, Encoder::Error> {
                #[derive(Serialize)]
                struct Fields<'a> {
                    $($(#[$attribute])* $field: &'a $value,)*
                }
                codex_object!(@require_any ser self [$first $(, $alternative)+]);
                Fields { $($field: &self.$field,)* }.serialize(encoder)
            }
        }
    };
    ($(#[$documentation:meta])* $name:ident {
        $($(#[$attribute:meta])* $field:ident: $value:ty,)*
    }) => {
        codex_object!(@record []
            $(#[$documentation])* $name { $($(#[$attribute])* $field: $value,)* }
        );
    };
    (@record [$(#[$constraint:meta])*]
        $(#[$documentation:meta])* $name:ident {
            $($(#[$attribute:meta])* $field:ident: $value:ty,)*
        }
    ) => {
        $(#[$documentation])*
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name {
            $($(#[$attribute])* pub $field: $value,)*
        }
        codex_object!(@decode [$(#[$constraint])*] [] $name { $($(#[$attribute])* $field: $value,)* });
    };
    (@decode [$(#[$constraint:meta])*] [$($alternative:ident),*] $name:ident {
        $($(#[$attribute:meta])* $field:ident: $value:ty,)*
    }) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                $(#[$constraint])*
                struct Fields {
                    $($(#[$attribute])* $field: $value,)*
                }
                let fields: Fields = super::deserialize_object(deserializer)?;
                codex_object!(@require_any de fields [$($alternative),*]);
                let Fields { $($field,)* } = fields;
                Ok(Self { $($field,)* })
            }
        }
    };
    (@require_any $direction:ident $value:ident []) => {};
    (@require_any $direction:ident $value:ident [$first:ident $(, $alternative:ident)+]) => {
        if !$value.$first.as_ref().is_some_and(super::StringAlternative::is_valid)
            $(&& !$value.$alternative.as_ref().is_some_and(super::StringAlternative::is_valid))+ {
            return Err(serde::$direction::Error::custom(concat!(
                "expected one or more valid string alternatives: ", stringify!($first), $(", ", stringify!($alternative)),+
            )));
        }
    };
}

// Keep the tagged enum's derived field decoder behind the same object-only entrance.
macro_rules! codex_tagged_object {
    ($(#[$documentation:meta])* $name:ident {
        $($tag:literal => $variant:ident($payload:ty),)+
    }) => {
        $(#[$documentation])*
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        #[serde(tag = "type")]
        pub enum $name {
            $(#[serde(rename = $tag)] $variant(Box<$payload>),)+
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<Decoder: serde::Deserializer<'de>>(
                decoder: Decoder,
            ) -> Result<Self, Decoder::Error> {
                #[derive(Deserialize)]
                #[serde(tag = "type")]
                enum Tagged {
                    $(#[serde(rename = $tag)] $variant(Box<$payload>),)+
                }
                match super::deserialize_object::<_, Tagged>(decoder)? {
                    $(Tagged::$variant(payload) => Ok(Self::$variant(payload)),)+
                }
            }
        }
    };
}

mod generated;
pub use generated::*;

fn deserialize_object<'de, D, Payload>(deserializer: D) -> Result<Payload, D::Error>
where
    D: Deserializer<'de>,
    Payload: Deserialize<'de>,
{
    struct ObjectVisitor<Payload>(PhantomData<Payload>);

    impl<'de, Payload: Deserialize<'de>> Visitor<'de> for ObjectVisitor<Payload> {
        type Value = Payload;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a Codex protocol object")
        }

        fn visit_map<A: MapAccess<'de>>(self, fields: A) -> Result<Payload, A::Error> {
            Payload::deserialize(MapAccessDeserializer::new(fields))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(PhantomData))
}

// Missing fields use serde's outer None. A present nullable field may then
// decode to Some(None), while a present non-nullable field rejects null.
fn deserialize_present<'de, D, Value>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
    Value: Deserialize<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

// A custom field decoder tells the derived struct decoder to reject a missing key.
fn deserialize_required_nullable<'de, D, Value>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
    Value: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}
