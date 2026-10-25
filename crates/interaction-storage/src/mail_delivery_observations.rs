//! Read-only native observations for subscription views, never allocation facts.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, ProviderAcpRouteEvidence, PushDeliveryState, PushHeaderFacts,
    PushId, PushRecord,
};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};

#[derive(Clone, Debug)]
pub struct MailDeliveryObservation {
    pub state: PushDeliveryState,
    pub held_since: Option<DateTime<Utc>>,
    pub scheduled_retry_at: Option<DateTime<Utc>>,
    pub last_outcome: Option<DeliveryReceipt>,
    pub recorded_at: Option<DateTime<Utc>>,
    pub route_evidence: Option<ProviderAcpRouteEvidence>,
}

/// This companion metadata commits with the SDK outcome and never changes its public receipt.
pub async fn record_mail_route_evidence_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    record: &PushRecord,
    evidence: Option<&ProviderAcpRouteEvidence>,
) -> Result<(), StorageError> {
    if evidence.is_some_and(|evidence| {
        !evidence.is_valid()
            || evidence.target != record.target
            || record.delivery_state != PushDeliveryState::OutcomeUnknown
            || !matches!(
                record.last_outcome.as_ref().map(|receipt| &receipt.outcome),
                Some(DeliveryOutcome::Unknown)
            )
    }) {
        return Err(StorageError::InvalidStoredRecord);
    }
    let encoded = evidence
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StorageError::InvalidStoredRecord)?;
    let changed = sqlx_turso::query!(
        "UPDATE router_pushes SET last_route_evidence_json=? WHERE push_id=? AND retention='mailTerminalAge'",
        encoded, record.push_id.as_str(),
    ).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    Ok(())
}

impl InteractionStore {
    pub async fn mail_delivery_observation(
        &self,
        item: &PushId,
    ) -> Result<Option<MailDeliveryObservation>, StorageError> {
        let mut connection = self.writer.lock().await;
        let row = sqlx_turso::query!(
            "SELECT p.delivery_state AS \"state!: String\",p.header_facts_json AS \"header!: String\",p.target_service_id AS \"service!: String\",p.target_endpoint_id AS \"endpoint!: String\",p.target_session_id AS \"session!: String\",p.last_outcome_json,p.last_outcome_at,p.last_route_evidence_json,r.scheduled_retry_at FROM router_pushes p LEFT JOIN local_delivery_retries r ON r.item_id=p.push_id WHERE p.push_id=? AND p.retention='mailTerminalAge'",
            item.as_str()
        ).fetch_optional(&mut *connection).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let state = push_record_rows::decode_delivery_state(&row.state)?;
        let header: PushHeaderFacts =
            serde_json::from_str(&row.header).map_err(|_| StorageError::InvalidStoredRecord)?;
        let is_follow = matches!(header, PushHeaderFacts::SubscriptionActivity { .. });
        let held_since = match header {
            PushHeaderFacts::SubscriptionActivity { held_since, .. } => held_since
                .map(|value| {
                    DateTime::parse_from_rfc3339(&value)
                        .map(|at| at.with_timezone(&Utc))
                        .map_err(|_| StorageError::InvalidStoredRecord)
                })
                .transpose()?,
            _ => None,
        };
        let scheduled_retry_at = row
            .scheduled_retry_at
            .map(push_record_rows::decode_timestamp)
            .transpose()?;
        let recorded_at = row
            .last_outcome_at
            .map(push_record_rows::decode_timestamp)
            .transpose()?;
        let last_outcome = row
            .last_outcome_json
            .map(|json| serde_json::from_str(&json).map_err(|_| StorageError::InvalidStoredRecord))
            .transpose()?;
        let route_evidence: Option<ProviderAcpRouteEvidence> = row
            .last_route_evidence_json
            .map(|value| {
                serde_json::from_str(&value).map_err(|_| StorageError::InvalidStoredRecord)
            })
            .transpose()?;
        if let Some(evidence) = &route_evidence {
            let target =
                push_record_rows::session_from_parts(row.service, row.endpoint, row.session)?;
            if evidence.target != target
                || state != PushDeliveryState::OutcomeUnknown
                || recorded_at.is_none()
            {
                return Err(StorageError::InvalidStoredRecord);
            }
        }
        let valid_outcome = match (
            state,
            last_outcome
                .as_ref()
                .map(|receipt: &DeliveryReceipt| &receipt.outcome),
        ) {
            (PushDeliveryState::Pending | PushDeliveryState::Attempted, None) => true,
            (
                PushDeliveryState::Held,
                Some(DeliveryOutcome::NotSubmitted {
                    retryable: true, ..
                }),
            ) => true,
            (
                PushDeliveryState::Held,
                Some(DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_)),
            ) => is_follow,
            (
                PushDeliveryState::Delivered,
                Some(
                    DeliveryOutcome::Started
                    | DeliveryOutcome::Steered
                    | DeliveryOutcome::StartedOrSteered
                    | DeliveryOutcome::Queued
                    | DeliveryOutcome::PeerMessageWritten,
                ),
            ) => true,
            (PushDeliveryState::OutcomeUnknown, Some(DeliveryOutcome::Unknown)) => true,
            (
                PushDeliveryState::Rejected,
                Some(DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_)),
            ) => true,
            _ => false,
        };
        // A released Pending item and an Attempted retry have cleared their
        // public receipt; the preceding observation time remains historical.
        if !valid_outcome
            || scheduled_retry_at.is_some()
                && !matches!(
                    state,
                    PushDeliveryState::Held | PushDeliveryState::Attempted
                )
            || recorded_at.is_some()
                && last_outcome.is_none()
                && !matches!(
                    state,
                    PushDeliveryState::Pending | PushDeliveryState::Attempted
                )
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        Ok(Some(MailDeliveryObservation {
            state,
            held_since,
            scheduled_retry_at,
            last_outcome,
            recorded_at,
            route_evidence,
        }))
    }
}
