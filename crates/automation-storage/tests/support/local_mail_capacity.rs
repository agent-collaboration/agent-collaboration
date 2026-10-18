//! A genuinely full inbox: every occupying item and carrier is admitted normally.
use automation_storage::{AutomationStore, LocalMessageRequest};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId};
use sqlx::SqliteConnection;

type FixtureResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
pub struct FullInbox {
    pub held: LocalMessageRequest,
    pub last: LocalMessageRequest,
}
pub async fn fill_local_inbox(
    store: &mut AutomationStore,
    observer: &mut SqliteConnection,
    template: LocalMessageRequest,
) -> FixtureResult<FullInbox> {
    const CAP: i64 = 256 * 1024 * 1024;
    let mut held = template;
    held.draft.body = Some("x".into());
    store.prepare_local_message(&held).await?;
    store
        .hold_unsent_direct_message(
            &held.draft.push_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    reason: "capacity fixture is unloaded".into(),
                    retryable: true,
                },
                reachability: None,
                client: None,
            },
        )
        .await?;
    let payload: i64 = sqlx::query_scalar("SELECT payload_bytes FROM mail_items WHERE item_id=?")
        .bind(held.draft.push_id.as_str())
        .fetch_one(&mut *observer)
        .await?;
    let overhead = payload - 1 + 2048;
    let largest = overhead + 2 * 65_536;
    let mut remaining = CAP - (overhead + 1);
    while remaining > 2 * largest {
        let item = sized_request(&held, 2 * 65_536)?;
        store.prepare_local_message(&item).await?;
        remaining -= largest;
    }
    // Split the tail so both real payloads fit the 64KiB decoded-text bound.
    let first_charge = remaining / 2;
    let tail = sized_request(&held, first_charge - overhead)?;
    store.prepare_local_message(&tail).await?;
    let last = sized_request(&held, remaining - first_charge - overhead)?;
    store.prepare_local_message(&last).await?;
    let reserved: i64 = sqlx::query_scalar(
        "SELECT SUM(payload_bytes+2048) FROM mail_items WHERE terminal_at IS NULL",
    )
    .fetch_one(observer)
    .await?;
    if reserved != CAP {
        return Err(format!("fixture occupancy {reserved} != {CAP}").into());
    }
    Ok(FullInbox { held, last })
}
fn sized_request(
    template: &LocalMessageRequest,
    encoded_text_bytes: i64,
) -> FixtureResult<LocalMessageRequest> {
    let bytes = usize::try_from(encoded_text_bytes)?;
    let mut body = "\t".repeat(bytes / 2);
    if bytes % 2 == 1 {
        body.push('x');
    }
    if body.is_empty() || body.len() > 65_536 {
        return Err("fixture text out of bounds".into());
    }
    let mut draft = template.draft.clone();
    draft.push_id = PushId::try_from(uuid::Uuid::now_v7().to_string())?;
    draft.body = Some(body);
    Ok(LocalMessageRequest {
        draft,
        target_identity: template.target_identity.clone(),
    })
}
