//! Codex app-server wire bindings from the pinned experimental schema and pairing.
//!
//! A plain JSON-RPC envelope can retain its `result` field as a
//! [`RawValue`]. After selecting and correlating the reply,
//! pass that field's JSON bytes to [`CodexRequest::decode_response`].
//! Decode request bodies directly from JSON text or bytes with Serde.
//!
//! An existing `serde_json::Value` can still decode a valid body. It may already
//! have rounded opaque numbers. Derived Serde envelopes that buffer payloads with
//! `untagged`, `flatten`, or internal tagging can reject valid bodies that
//! require access to raw JSON.

use serde::de::{DeserializeOwned, MapAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use std::{fmt, marker::PhantomData};

mod sealed {
    pub trait Sealed {}
}

/// A Codex client request body and its upstream-declared reply body.
pub trait CodexRequest: sealed::Sealed + Serialize + DeserializeOwned {
    const METHOD: &'static str;
    type Response: Serialize + DeserializeOwned;

    /// Decodes the complete JSON payload of a selected, correlated JSON-RPC `result`.
    ///
    /// Pass the bytes of the raw result, for example `result.get().as_bytes()`
    /// from a `Box<RawValue>` field on a plain envelope. The caller owns envelope
    /// selection, reply correlation, and JSON-RPC errors.
    ///
    /// # Errors
    ///
    /// Returns a JSON error for an invalid response body or trailing non-whitespace data.
    fn decode_response(result_json: &[u8]) -> serde_json::Result<Self::Response> {
        serde_json::from_slice(result_json)
    }
}

/// A validated JSON value in a schema-declared opaque field.
///
/// Decode directly from JSON text or bytes and serialize back to JSON to retain the source
/// numbers. An intermediate `serde_json::Value` may have already rounded them.
///
/// Equality compares the retained JSON text. Whitespace and key order inside the value
/// can therefore affect equality of enclosing generated types.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct OpaqueJson(Box<RawValue>);

impl OpaqueJson {
    /// The validated JSON representation, including the original number tokens.
    #[must_use]
    pub fn as_json(&self) -> &str {
        self.0.get()
    }
}

impl PartialEq for OpaqueJson {
    fn eq(&self, other: &Self) -> bool {
        self.as_json() == other.as_json()
    }
}

impl Eq for OpaqueJson {}

/// Opaque JSON whose enclosing wire shape must be an object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpaqueJsonObject(OpaqueJson);

impl OpaqueJsonObject {
    #[must_use]
    pub fn as_json(&self) -> &str {
        self.0.as_json()
    }
}

impl TryFrom<OpaqueJson> for OpaqueJsonObject {
    type Error = &'static str;

    fn try_from(value: OpaqueJson) -> Result<Self, Self::Error> {
        if value.as_json().starts_with('{') {
            Ok(Self(value))
        } else {
            Err("expected an opaque JSON object")
        }
    }
}

impl<'de> Deserialize<'de> for OpaqueJsonObject {
    fn deserialize<Decoder: Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {
        Self::try_from(OpaqueJson::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

/// A supplied string alternative in a payload that requires at least one valid alternative.
/// A different valid alternative can admit the payload even when this value is malformed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StringAlternative {
    Valid(String),
    Malformed(OpaqueJson),
}

impl<'de> Deserialize<'de> for StringAlternative {
    fn deserialize<Decoder: Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {
        let wire = OpaqueJson::deserialize(decoder)?;
        if wire.as_json().starts_with('"') {
            serde_json::from_str(wire.as_json())
                .map(Self::Valid)
                .map_err(serde::de::Error::custom)
        } else {
            Ok(Self::Malformed(wire))
        }
    }
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

// Serde's internally tagged decoder buffers fields with deserialize_any, which can round an
// opaque number before its field decoder runs. Read the tag and typed payload from the JSON.
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
                let wire = super::OpaqueJson::deserialize(decoder)?;
                let tag = super::json_object_tag(wire.as_json()).map_err(serde::de::Error::custom)?;
                match tag.as_str() {
                    $($tag => serde_json::from_str::<Box<$payload>>(wire.as_json())
                        .map(Self::$variant).map_err(serde::de::Error::custom),)+
                    _ => Err(serde::de::Error::unknown_variant(&tag, &[$($tag),+])),
                }
            }
        }
    };
}

mod generated;
pub use generated::*;

fn json_object_tag(wire: &str) -> serde_json::Result<String> {
    #[derive(Deserialize)]
    struct Discriminator {
        #[serde(rename = "type")]
        tag: String,
    }

    let mut decoder = serde_json::Deserializer::from_str(wire);
    let fields: Discriminator = deserialize_object(&mut decoder)?;
    Ok(fields.tag)
}

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
