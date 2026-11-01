//! Schema constraints for validated identities; byte limits remain explicit codec obligations.
use crate::{
    EndpointId, GenerationNumber, NonEmptyText, ObservationTimestamp, SchemaDigest, SessionId,
    UuidIdentity,
};
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
    json_schema!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
);
validated_schema!(
    EndpointId,
    json_schema!({"type":"string","minLength":1,"maxLength":crate::limits::ENDPOINT_ID_MAX_BYTES,
        "pattern":format!("^[a-z][a-z0-9-]{{0,{}}}$", crate::limits::ENDPOINT_ID_MAX_BYTES - 1)})
);
validated_schema!(
    SessionId,
    json_schema!({"type":"string","minLength":1,"maxLength":crate::limits::SESSION_ID_MAX_BYTES,"pattern":"^[^\\u0000]+$","x-maxUtf8Bytes":crate::limits::SESSION_ID_MAX_BYTES})
);
validated_schema!(
    NonEmptyText,
    json_schema!({"type":"string","minLength":1,"maxLength":crate::limits::TEXT_MAX_BYTES,"pattern":"^[^\\u0000]+$","x-maxUtf8Bytes":crate::limits::TEXT_MAX_BYTES})
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
    json_schema!({"type":"integer","minimum":1,"maximum":crate::limits::JSON_SAFE_INTEGER_MAX})
);
