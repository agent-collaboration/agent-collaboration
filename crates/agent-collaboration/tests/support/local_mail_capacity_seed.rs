//! Pending SDK custody supplies occupancy; served requests prove the final boundary.
use super::super::native_sql::SqlHost;
use super::{LOCAL_INBOX_BYTES, wire_charge};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId, PushRecordDraft};
use durable_steps::{AtomicStore, StoreError, turso_store::TursoScope};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox, LocalDelivery,
    LocalItemAdmission, LocalOutcome, minimum_settlement_allowance,
};
use fleet_sync_protocol::{DeliveryItem, DeliveryItemRef, PeerId};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::{CallId, SyncTime};
use std::sync::Arc;

type RouterTypes = collaboration_contracts::RouterDeliveryTypes;

struct PendingSeedConsumer {
    now: chrono::DateTime<chrono::Utc>,
}

impl DeliveryConsumer<TursoScope<'_, SqlHost>> for PendingSeedConsumer {
    type Protocol = collaboration_contracts::RouterConsumerProtocol;

    async fn on_received(
        &self,
        scope: &mut TursoScope<'_, SqlHost>,
        item: &DeliveryItem<RouterTypes>,
    ) -> Result<(), DeliveryError> {
        collaboration_runtime::project_received_mail(scope, item, self.now).await
    }

    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, SqlHost>,
        _: &DeliveryItem<RouterTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Hold { until: None })
    }

    async fn deliver_locally(
        &self,
        _: &DeliveryItem<RouterTypes>,
        _: CallId,
    ) -> LocalDelivery<RouterTypes> {
        LocalDelivery {
            outcome: LocalOutcome::Uncertain,
            observation: DeliveryReceipt {
                outcome: DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            }
            .into(),
        }
    }

    fn may_cancel(
        &self,
        sender: &<RouterTypes as fleet_sync_protocol::DeliveryTypes>::Sender,
        item: &fleet_sync_protocol::DeliveryHeader<RouterTypes>,
    ) -> bool {
        sender == item.sender()
    }
}

pub(super) struct SeededInbox {
    pub count: u32,
    pub bytes: u64,
    pub first: PushId,
    pub last: PushId,
    pub first_body: String,
    pub last_body: String,
}

fn fit_seed_body(
    draft: &PushRecordDraft,
    peer: PeerId,
    budget: u64,
    ordinary_reserve_bytes: u32,
) -> Result<Option<(PushRecordDraft, u64)>, StoreError> {
    let body = draft
        .body
        .as_ref()
        .ok_or_else(|| StoreError::backend("seed body absent"))?;
    let mut lower = 1;
    let mut upper = body.chars().count();
    let mut fitted = None;
    while lower <= upper {
        let middle = lower + (upper - lower) / 2;
        let mut candidate = draft.clone();
        candidate.body = Some(body.chars().take(middle).collect());
        let charge = wire_charge(&candidate, peer, ordinary_reserve_bytes)
            .map_err(|error| StoreError::backend(error.to_string()))?;
        if charge <= budget {
            fitted = Some((candidate, charge));
            lower = middle + 1;
        } else {
            upper = middle - 1;
        }
    }
    Ok(fitted)
}

pub(super) async fn seed_to_last_slot(
    owner: Arc<InteractionStore>,
    peer: PeerId,
    prototype: PushRecordDraft,
    already_used: u64,
    final_charge: u64,
    ordinary_reserve_bytes: u32,
) -> Result<SeededInbox, StoreError> {
    let reserve = minimum_settlement_allowance::<RouterTypes>().map_err(StoreError::backend)?;
    if ordinary_reserve_bytes == 0 {
        return Err(StoreError::backend(
            "fixture requires a positive settlement reserve",
        ));
    }
    let limits = DeliveryLimits::new::<RouterTypes>(
        1024 * 1024 * 1024,
        200_000,
        256 * 1024 * 1024,
        50_000,
        LOCAL_INBOX_BYTES,
        reserve,
    )
    .map_err(StoreError::backend)?;
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                let consumer = PendingSeedConsumer {
                    now: chrono::Utc::now(),
                };
                let now = SyncTime::from_unix_millis(consumer.now.timestamp_millis());
                let service = super::super::local_mail_surfaces::SERVICE
                    .to_owned()
                    .try_into()
                    .map_err(StoreError::backend)?;
                let mut used = already_used;
                let mut count = 0_u32;
                let mut first = None;
                let mut last = None;
                let mut first_body = None;
                let mut last_body = None;
                // IDs have a fixed UUID width and every seed uses this same
                // timestamp and body. Serialize their full-size charge once;
                // the last partial slot still measures its fitted envelope.
                let mut prototype = prototype;
                prototype.created_at = consumer.now;
                let full_charge = wire_charge(&prototype, peer, ordinary_reserve_bytes)
                    .map_err(|error| StoreError::backend(error.to_string()))?;
                // Every seed has the same ordinary profile as the observed prototype.
                let maximum_items = LOCAL_INBOX_BYTES / u64::from(ordinary_reserve_bytes);
                for _ in 0..=maximum_items {
                    let mut draft = prototype.clone();
                    draft.push_id = collaboration_protocol::MailItemId::generate()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    draft.created_at = consumer.now;
                    let mut charge = full_charge;
                    let budget = LOCAL_INBOX_BYTES
                        .checked_sub(used)
                        .and_then(|remaining| remaining.checked_sub(final_charge))
                        .ok_or_else(|| StoreError::backend("final served item exceeds quota"))?;
                    // Different valid modes change the literal envelope charge.
                    // Fit one real pending seed into the last partial slot rather
                    // than assuming the full prototype divides the production cap.
                    if charge > budget
                        && budget >= u64::from(ordinary_reserve_bytes)
                        && let Some((fitted, fitted_charge)) =
                            fit_seed_body(&draft, peer, budget, ordinary_reserve_bytes)?
                    {
                        draft = fitted;
                        charge = fitted_charge;
                    }
                    let next = used
                        .checked_add(charge)
                        .ok_or_else(|| StoreError::backend("fixture occupancy overflow"))?;
                    if next
                        .checked_add(final_charge)
                        .is_none_or(|total| total > LOCAL_INBOX_BYTES)
                    {
                        let first: DeliveryItemRef = first
                            .ok_or_else(|| StoreError::backend("no pending custody was seeded"))?;
                        let last: DeliveryItemRef =
                            last.ok_or_else(|| StoreError::backend("last seeded item missing"))?;
                        let mut inbox = Inbox::<_, RouterTypes>::new(scope, peer, limits);
                        for reference in [&first, &last] {
                            if inbox
                                .local_outcome(reference.item)
                                .await
                                .map_err(StoreError::backend)?
                                .is_some()
                            {
                                return Err(StoreError::backend(
                                    "seeded occupancy was not pending SDK custody",
                                ));
                            }
                            inbox
                                .resolve_delivery(reference)
                                .await
                                .map_err(StoreError::backend)?;
                        }
                        return Ok(SeededInbox {
                            count,
                            bytes: used,
                            first: first
                                .item
                                .as_uuid()
                                .to_string()
                                .try_into()
                                .map_err(StoreError::backend)?,
                            last: last
                                .item
                                .as_uuid()
                                .to_string()
                                .try_into()
                                .map_err(StoreError::backend)?,
                            first_body: first_body
                                .ok_or_else(|| StoreError::backend("first seed body absent"))?,
                            last_body: last_body
                                .ok_or_else(|| StoreError::backend("last seed body absent"))?,
                        });
                    }
                    let item = collaboration_runtime::outgoing_mail_message(&draft, &service, peer)
                        .map_err(StoreError::backend)?;
                    let reference = DeliveryItemRef {
                        item: item.id(),
                        digest: item.digest().clone(),
                    };
                    // Local enqueue does not invoke receive projection: use the actual hook
                    // explicitly in this same transaction, before SDK custody admission.
                    consumer
                        .on_received(scope, &item)
                        .await
                        .map_err(StoreError::backend)?;
                    let admission = Inbox::<_, RouterTypes>::new(scope, peer, limits)
                        .enqueue_local(item, &consumer, now)
                        .await
                        .map_err(StoreError::backend)?;
                    if !matches!(admission, LocalItemAdmission::Attempt(_)) {
                        return Err(StoreError::backend("fresh SDK occupancy was not admitted"));
                    }
                    used = next;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| StoreError::backend("fixture item count overflow"))?;
                    first.get_or_insert_with(|| reference.clone());
                    last = Some(reference);
                    let body = draft
                        .body
                        .ok_or_else(|| StoreError::backend("seed body absent"))?;
                    first_body.get_or_insert_with(|| body.clone());
                    last_body = Some(body);
                }
                Err(StoreError::backend(
                    "fixture exceeded the byte quota's derived item bound",
                ))
            })
        })
        .await
}
