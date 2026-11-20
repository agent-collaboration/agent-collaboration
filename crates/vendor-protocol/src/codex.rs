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

// Serde's derived struct decoder also accepts positional sequences. Codex's
// object schemas only admit maps. Reuse the derived field decoder on that path.
macro_rules! codex_object {
    ($(#[$documentation:meta])* $name:ident {
        $($(#[$attribute:meta])* $field:ident: $value:ty,)*
    }) => {
        $(#[$documentation])*
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name {
            $($(#[$attribute])* pub $field: $value,)*
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                struct Fields {
                    $($(#[$attribute])* $field: $value,)*
                }
                let fields: Fields = super::deserialize_object(deserializer)?;
                Ok(Self { $($field: fields.$field,)* })
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
