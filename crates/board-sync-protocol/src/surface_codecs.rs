//! Remote derives keep the library's values authoritative; only names change.
use fleet_sync_protocol as wire;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::PieceRefusal",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PieceRefusalCodec {
    StaleEpoch {
        newer: wire::Epoch,
        ended_at: wire::ReplicationPosition,
    },
    NotWriter {
        current: Option<wire::Epoch>,
    },
    #[serde(rename = "positionGap")]
    Gap {
        acked: wire::ReplicationPosition,
    },
    PieceConflict,
    SchemaUnsupported {
        form: wire::SchemaStatementForm,
    },
    NeedsSnapshot {
        acked: wire::ReplicationPosition,
    },
    IntegrityViolation {
        table: wire::TableName,
        references: wire::TableName,
    },
    NotInitialized,
    #[serde(rename = "centralCopyFull")]
    CopyFull,
    Retired,
    Hook(wire::HookRefusal),
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::LeaseRefusal",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum LeaseRefusalCodec {
    StaleEpoch {
        current_epoch: wire::Epoch,
        epoch_ended_at: wire::ReplicationPosition,
    },
    NotWriter {
        current: Option<wire::Epoch>,
    },
    NoAuthoritativeCopy,
    #[serde(rename = "registryEpochUnproven")]
    EpochUnprovable,
    NotRegistered,
    Retired,
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::RegistrationRefusal",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum RegistrationRefusalCodec {
    #[serde(rename = "projectRejected")]
    DatabaseRejected {
        reason: wire::GateReason,
    },
    NotWriter {
        current: Option<wire::Epoch>,
    },
    Retired,
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::HolderChangeAnswer",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ClaimAnswerCodec {
    Cancelled {
        by: wire::ActorRef,
    },
    #[serde(rename = "claimed")]
    Granted {
        epoch: wire::Epoch,
        expires_at: wire::SyncTime,
        start: wire::ReplicationPosition,
    },
    Pending {
        transfer: wire::TransferId,
    },
    Refused(#[serde(with = "holder_refusal_value")] wire::HolderChangeRefusal),
    Failed {
        reason: wire::FailureCode,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::DeliveryStatus",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum DeliveryStatusCodec<A> {
    Queued,
    Sent {
        seq: wire::DeliverySeq,
    },
    #[serde(rename = "atMachine")]
    AtRecipient,
    Held,
    Delivered {
        acceptance: A,
    },
    Uncertain,
    Expired,
    Cancelled,
    Refused {
        reason: wire::DeliveryRefusal,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "wire::CancelResult",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CancelResultCodec {
    #[serde(rename = "cancelPending")]
    Pending,
    Prevented,
    TooLate {
        at: wire::TooLateAt,
    },
}

mod holder_refusal_value {
    use crate::BoardHolderChangeRefusal;
    use fleet_sync_protocol::HolderChangeRefusal;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        value: &HolderChangeRefusal,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        BoardHolderChangeRefusal::from(value.clone()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HolderChangeRefusal, D::Error> {
        BoardHolderChangeRefusal::deserialize(deserializer).map(Into::into)
    }
}

#[cfg(test)]
mod integrity_refusal_tests {

    use super::*;
    #[derive(Serialize, Deserialize)]
    struct Refusal(#[serde(with = "PieceRefusalCodec")] wire::PieceRefusal);

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[test]
    fn integrity_refusal_surface_preserves_the_typed_relationship()
    -> Result<(), Box<dyn std::error::Error>> {
        let refusal = Refusal(wire::PieceRefusal::IntegrityViolation {
            table: wire::TableName::new("posts")?,
            references: wire::TableName::new("threads")?,
        });
        let encoded = serde_json::to_value(&refusal)?;
        assert_eq!(
            encoded,
            serde_json::json!({"kind":"integrityViolation","value":{"table":"posts","references":"threads"}})
        );
        let decoded: Refusal = serde_json::from_value(encoded)?;
        assert_eq!(decoded.0, refusal.0);
        Ok(())
    }
}
