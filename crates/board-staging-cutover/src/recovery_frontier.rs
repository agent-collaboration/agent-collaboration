//! Account for the exact singleton frontier and prove every newly covered recovery handoff.
use crate::intent_recovery_rows::{
    RecoveryDelta, RecoveryError, RecoveryResult, integer, json, require, text,
};
use crate::recovery_custody::RecoveryCustody;
use fleet_protocol::{DeliveryEnvelope, PeerId};
use message_board::ServiceId;
use message_board_storage::MailHandoffItem;
use std::path::Path;

#[cfg(test)]
#[path = "recovery_frontier_test.rs"]
mod tests;

pub(crate) async fn verify_frontier(
    delta: &RecoveryDelta,
    path: &Path,
    service: &ServiceId,
    peer: PeerId,
) -> RecoveryResult<()> {
    let source = delta.source_frontier.as_ref();
    let added = delta.added("handoff_frontier");
    let actual = match (source, &delta.changed_frontier, added) {
        (Some(row), None, []) => Some(row),
        (Some(old), Some(new), []) => {
            require(
                text(old, 0)? == text(new, 0)?,
                "preserved frontier identity",
            )?;
            Some(new)
        }
        (None, None, []) => None,
        (None, None, [row]) => {
            let id = uuid::Uuid::parse_str(text(row, 0)?)
                .map_err(|_| RecoveryError::Invalid("frontier identifier"))?;
            require(runtime_primitives::uuid_v7(id), "frontier UUIDv7")?;
            Some(row)
        }
        _ => return Err(RecoveryError::Invalid("frontier singleton")),
    };
    let before = source.map(|row| integer(row, 1)).transpose()?.unwrap_or(0);
    let after = actual.map(|row| integer(row, 1)).transpose()?.unwrap_or(0);
    let mut head = delta.source_handoff_head;
    for row in delta.added("mail_handoffs") {
        head = head.max(integer(row, 1)?);
    }
    require(
        before >= 0 && after >= before && after <= head,
        "frontier range",
    )?;
    if before == after {
        return Ok(());
    }
    let mut advanced = Vec::new();
    for row in delta.added("mail_handoffs") {
        let sequence = integer(row, 1)?;
        if sequence > before && sequence <= after {
            advanced.push(row);
        }
    }
    require(
        i64::try_from(advanced.len()).ok() == after.checked_sub(before),
        "frontier covers an old or absent handoff",
    )?;
    let mut custody = RecoveryCustody::open(path, service, peer).await?;
    let checked = async {
        for row in advanced {
            let MailHandoffItem::Delivery(envelope) = json(row, 2)? else {
                return Err(RecoveryError::Invalid("frontier cancel group"));
            };
            let DeliveryEnvelope::Item(item) = *envelope else {
                return Err(RecoveryError::Invalid("frontier cancel item"));
            };
            require(
                item.id().as_uuid().to_string() == text(row, 0)?
                    && item.recipient().peer == peer
                    && item.recipient().address.endpoint.service_id == *service,
                "frontier local handoff",
            )?;
            custody
                .terminal(item.id(), &item.recipient().address)
                .await?;
        }
        Ok(())
    }
    .await;
    let closed = custody.close().await;
    checked?;
    closed
}
