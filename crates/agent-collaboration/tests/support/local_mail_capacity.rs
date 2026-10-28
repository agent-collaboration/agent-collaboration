//! Real SDK quota admission checked against declared capacity using public wire sizes.
use super::local_mail_surfaces::{Surfaces, TestResult};
use collaboration_protocol::{MailItemId, PushId, PushRecordDraft};
use durable_steps::{AtomicStore, StoreError};
use serde_json::json;
#[path = "local_mail_capacity_seed.rs"]
mod capacity_seed;

// PD4 §3's actual Local byte quota, independent of the observed refusal.
const LOCAL_INBOX_BYTES: u64 = 256 * 1024 * 1024;

fn wire_charge(
    draft: &PushRecordDraft,
    peer: fleet_sync_protocol::PeerId,
    ordinary_reserve_bytes: u32,
) -> TestResult<u64> {
    let item = collaboration_runtime::outgoing_mail_message(
        draft,
        &super::local_mail_surfaces::SERVICE.to_owned().try_into()?,
        peer,
    )?;
    let payload = u64::try_from(
        serde_json::to_vec(&fleet_sync_protocol::DeliveryEnvelope::Item(item))?.len(),
    )?;
    payload
        .checked_add(u64::from(ordinary_reserve_bytes))
        .ok_or_else(|| "fixture charge overflow".into())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredOrdinaryReserve {
    acceptance_bound: u32,
    settlement_bytes: u32,
}

async fn observe_ordinary_reserve(surfaces: &Surfaces, item: &PushId) -> TestResult<u32> {
    let identity = item.as_str().to_owned();
    // This fixture projects the public snapshot's acceptanceBound/settlementBytes
    // JSON fields; the SDK still owns the reserve formula and private queries.
    let observed = interaction_storage::InteractionAtomicStore::new(
        surfaces.interactions.clone(),
        std::sync::Arc::new(super::native_sql::SqlHost),
    )
    .transaction(move |scope| {
        Box::pin(async move {
            let row = fleet_sync_delivery::test_support::snapshot(scope)
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .find(|row| row.category == "inbox" && row.identity == identity)
                .and_then(|row| row.content)
                .ok_or_else(|| StoreError::backend("ordinary inbox snapshot header missing"))?;
            serde_json::from_str::<StoredOrdinaryReserve>(&row).map_err(StoreError::backend)
        })
    })
    .await?;
    if observed.acceptance_bound != 64 || observed.settlement_bytes == 0 {
        return Err("ordinary item must retain acceptanceBound64 and a positive reserve".into());
    }
    Ok(observed.settlement_bytes)
}

pub struct FullInbox {
    pub first: PushId,
    pub last: PushId,
    pub body: String,
    pub admitted: u32,
}

async fn served_held(surfaces: &Surfaces, body: &str) -> TestResult<PushRecordDraft> {
    let id = MailItemId::generate();
    let input = Surfaces::held_input(Some(id.as_str()), body);
    let reply = surfaces.api.call_structured("message_send", input).await?;
    if reply.pointer("/result/deliveryState") != Some(&json!("held")) {
        return Err(
            format!("served capacity input did not enter real SDK Held custody: {reply}").into(),
        );
    }
    let record = surfaces
        .interactions
        .get_push_record(&id.try_into()?)
        .await?
        .ok_or("served admission has no native projection")?;
    Ok(super::local_mail_surfaces::draft_from_saved_push(record))
}

pub async fn fill_local_inbox(surfaces: &Surfaces) -> TestResult<FullInbox> {
    let started = std::time::Instant::now();
    let body = "\t".repeat(65_536);
    // The first and final items traverse the served application and really become Held.
    let first = served_held(surfaces, &body).await?;
    let ordinary_reserve_bytes = observe_ordinary_reserve(surfaces, &first.push_id).await?;
    let first_charge = wire_charge(&first, surfaces.peer, ordinary_reserve_bytes)?;
    let seeded = capacity_seed::seed_to_last_slot(
        surfaces.interactions.clone(),
        surfaces.peer,
        first.clone(),
        first_charge,
        first_charge,
        ordinary_reserve_bytes,
    )
    .await?;
    for (id, expected_body) in [
        (&seeded.first, &seeded.first_body),
        (&seeded.last, &seeded.last_body),
    ] {
        let record = surfaces
            .interactions
            .get_push_record(id)
            .await?
            .ok_or("SDK occupancy has no received projection")?;
        if record.delivery_state != collaboration_protocol::PushDeliveryState::Pending
            || record.body.as_ref() != Some(expected_body)
        {
            return Err("seeded occupancy changed its pending state or full body".into());
        }
    }
    eprintln!(
        "local quota pending seed: count={} wire_bytes={} elapsed_ms={}",
        seeded.count,
        seeded.bytes,
        started.elapsed().as_millis()
    );
    let last = served_held(surfaces, &body).await?;
    let used = seeded
        .bytes
        .checked_add(wire_charge(&last, surfaces.peer, ordinary_reserve_bytes)?)
        .ok_or("fixture occupancy overflow")?;
    let remaining = LOCAL_INBOX_BYTES
        .checked_sub(used)
        .ok_or("SDK admitted beyond the declared production quota")?;
    if remaining >= u64::from(ordinary_reserve_bytes) {
        return Err(format!(
            "capacity fixture left {remaining} bytes, at least one settlement reserve"
        )
        .into());
    }
    let before = super::native_sql::counts(surfaces.interactions.clone()).await?;
    let id = MailItemId::generate();
    let input = Surfaces::held_input(Some(id.as_str()), &body);
    let refused = surfaces.api.call_structured("message_send", input).await?;
    if refused.pointer("/error/serviceKind") != Some(&json!("mailboxFull"))
        || refused.pointer("/error/effect") != Some(&json!("none"))
    {
        return Err(format!(
            "served request crossed the real quota without a no-effect refusal: {refused}"
        )
        .into());
    }
    if super::native_sql::counts(surfaces.interactions.clone()).await? != before {
        return Err("quota refusal created an answer or push projection".into());
    }
    let admitted = seeded
        .count
        .checked_add(2)
        .ok_or("fixture item count overflow")?;
    eprintln!(
        "local quota served boundary: admitted={admitted} wire_bytes={used} remaining={remaining} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(FullInbox {
        first: first.push_id,
        last: last.push_id,
        body,
        admitted,
    })
}
