//! Schema constraints for validated identities; byte limits remain explicit codec obligations.
use crate::{
    EndpointId, GenerationNumber, NonEmptyText, ObservationTimestamp, SchemaDigest, SessionId,
    UuidIdentity,
};
use message_board::{CANONICAL_UUID_PATTERN, ENDPOINT_ID_PATTERN, OPAQUE_ID_PATTERN};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use std::borrow::Cow;

macro_rules! validated_schema {
    ($identity:ty, $schema:expr) => {
        impl JsonSchema for $identity {
            fn schema_name() -> Cow<'static, str> {
                stringify!($identity).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                $schema
            }
        }
    };
}
validated_schema!(
    UuidIdentity,
    json_schema!({"type":"string","pattern":CANONICAL_UUID_PATTERN})
);
validated_schema!(
    EndpointId,
    json_schema!({"type":"string","pattern":ENDPOINT_ID_PATTERN})
);
validated_schema!(
    SessionId,
    json_schema!({"type":"string","minLength":1,"maxLength":4096,"pattern":OPAQUE_ID_PATTERN,"x-maxUtf8Bytes":4096})
);
validated_schema!(
    NonEmptyText,
    json_schema!({"type":"string","minLength":1,"maxLength":4096,"pattern":OPAQUE_ID_PATTERN,"x-maxUtf8Bytes":4096})
);
validated_schema!(
    SchemaDigest,
    json_schema!({"type":"string","pattern":"^sha256:[0-9a-f]{64}$"})
);
validated_schema!(
    ObservationTimestamp,
    json_schema!({"type":"string","format":"date-time","pattern":"Z$"})
);
validated_schema!(
    GenerationNumber,
    json_schema!({"type":"integer","minimum":1,"maximum":9007199254740991_u64})
);
