//! Exact hibernatable transport state; session truth stays in the shared SQL store.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::arithmetic_side_effects,
    clippy::unimplemented,
    clippy::unreachable
)]

use collaboration_central_core::AdmissionOrigin;
use fleet_sync_protocol::{AttachSequence, ConnectionId, PeerId, SchemaSupport, SessionId};
use fleet_sync_session::{ConnectionBinding, ConnectionRegistration, VerifiedPeer};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WireBindingAttachment", into = "WireBindingAttachment")]
pub(crate) struct BindingAttachment {
    pub connection: ConnectionId,
    pub peer: PeerId,
    pub expires_at: SyncTime,
    pub registration: ConnectionRegistration,
    pub admission_origin: AdmissionOrigin,
}
impl BindingAttachment {
    pub(crate) fn binding(&self) -> ConnectionBinding {
        ConnectionBinding::new(
            self.connection,
            VerifiedPeer {
                peer: self.peer,
                expires_at: self.expires_at,
            },
        )
    }
}

// workers-rs uses serde_wasm_bindgen's default serializer, which cannot carry
// full i64/u64 domains as JS numbers. These private wire shapes turn every exact
// integer into canonical decimal text and validate it on reconstruction.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireBindingAttachment {
    connection: ConnectionId,
    peer: PeerId,
    expires_at: String,
    registration: WireRegistration,
    admission_origin: AdmissionOrigin,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum WireRegistration {
    Opening,
    Attaching {
        session_id: SessionId,
        schema_support: String,
        attach_seq: String,
    },
    Attached {
        session_id: SessionId,
        schema_support: String,
    },
    Closed,
}
#[derive(Debug, thiserror::Error)]
#[error("invalid socket attachment field: {field}")]
pub(crate) struct AttachmentError {
    field: &'static str,
}

fn exact_decimal<Value: std::str::FromStr + std::fmt::Display>(
    text: String,
    field: &'static str,
) -> Result<Value, AttachmentError> {
    let value = text
        .parse::<Value>()
        .map_err(|_| AttachmentError { field })?;
    if value.to_string() != text {
        return Err(AttachmentError { field });
    }
    Ok(value)
}
impl From<BindingAttachment> for WireBindingAttachment {
    fn from(attachment: BindingAttachment) -> Self {
        let registration = match attachment.registration {
            ConnectionRegistration::Opening => WireRegistration::Opening,
            ConnectionRegistration::Attaching {
                session_id,
                schema_support,
                attach_seq,
            } => WireRegistration::Attaching {
                session_id,
                schema_support: schema_support.max_version.to_string(),
                attach_seq: attach_seq.value().to_string(),
            },
            ConnectionRegistration::Attached {
                session_id,
                schema_support,
            } => WireRegistration::Attached {
                session_id,
                schema_support: schema_support.max_version.to_string(),
            },
            ConnectionRegistration::Closed => WireRegistration::Closed,
        };
        Self {
            connection: attachment.connection,
            peer: attachment.peer,
            expires_at: attachment.expires_at.unix_millis().to_string(),
            registration,
            admission_origin: attachment.admission_origin,
        }
    }
}
impl TryFrom<WireBindingAttachment> for BindingAttachment {
    type Error = AttachmentError;
    fn try_from(wire: WireBindingAttachment) -> Result<Self, Self::Error> {
        let registration = match wire.registration {
            WireRegistration::Opening => ConnectionRegistration::Opening,
            WireRegistration::Attaching {
                session_id,
                schema_support,
                attach_seq,
            } => ConnectionRegistration::Attaching {
                session_id,
                schema_support: SchemaSupport {
                    max_version: exact_decimal(schema_support, "schemaSupport")?,
                },
                attach_seq: AttachSequence::new(exact_decimal(attach_seq, "attachSeq")?)
                    .map_err(|_| AttachmentError { field: "attachSeq" })?,
            },
            WireRegistration::Attached {
                session_id,
                schema_support,
            } => ConnectionRegistration::Attached {
                session_id,
                schema_support: SchemaSupport {
                    max_version: exact_decimal(schema_support, "schemaSupport")?,
                },
            },
            WireRegistration::Closed => ConnectionRegistration::Closed,
        };
        Ok(Self {
            connection: wire.connection,
            peer: wire.peer,
            expires_at: SyncTime::from_unix_millis(exact_decimal(wire.expires_at, "expiresAt")?),
            registration,
            admission_origin: wire.admission_origin,
        })
    }
}
