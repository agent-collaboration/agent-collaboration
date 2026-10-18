//! MS-7 admission against an occupied real SQLite inbox.
use automation_storage::{AutomationStore, LocalMessageAdmission, StorageError};
use chrono::Utc;
use collaboration_mail_protocol::{MailRefusal, MailStatus};
use sqlx::Connection;
#[path = "support/local_mail_capacity.rs"]
mod local_mail_capacity;
#[path = "support/local_mail_fixture.rs"]
mod local_mail_fixture;
use local_mail_fixture::{Fixture, accepted, mail_id, request};
type TestResult = Result<(), Box<dyn std::error::Error>>;
macro_rules! ensure {
    ($condition:expr $(,)?) => {
        if !$condition {
            return Err(format!("assertion failed: {}", stringify!($condition)).into());
        }
    };
}
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!("left={left:?}, right={right:?}").into());
        }
    }};
}
#[tokio::test]
async fn occupied_inbox_admits_exactly_the_last_item_and_compaction_releases_its_budget()
-> TestResult {
    let fixture = Fixture::new();
    let mut store = AutomationStore::open(&fixture.path).await?;
    let mut observer = fixture.observer().await?;
    let full =
        local_mail_capacity::fill_local_inbox(&mut store, &mut observer, request(Utc::now())?)
            .await
            .map_err(|error| error.to_string())?;
    ensure!(
        store
            .read_local_mail(&mail_id(&full.last)?)
            .await?
            .is_some()
    );
    let rejected = request(Utc::now())?;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_items")
        .fetch_one(&mut observer)
        .await?;
    ensure!(matches!(
        store.prepare_local_message(&rejected).await,
        Err(StorageError::MailRefused(MailRefusal::MailboxFull {}))
    ));
    ensure!(store.read_local_mail(&mail_id(&rejected)?).await?.is_none());
    ensure!(
        store
            .get_push_record(&rejected.draft.push_id)
            .await?
            .is_none()
    );
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_items")
        .fetch_one(&mut observer)
        .await?;
    ensure_eq!(before, after);
    let held = store
        .read_local_mail(&mail_id(&full.held)?)
        .await?
        .ok_or("held item lost")?;
    ensure!(matches!(held.status, MailStatus::Held {}));
    ensure!(!held.compacted);
    let replay = store.prepare_local_message(&full.held).await?;
    let LocalMessageAdmission::Replayed(replay) = replay else {
        return Err("full inbox lost known-ID replay".into());
    };
    ensure_eq!(
        serde_json::to_value(replay.receipt)?,
        serde_json::to_value(held.result.ok_or("held result")?.receipt)?
    );
    ensure_eq!(
        store
            .get_push_record(&full.held.draft.push_id)
            .await?
            .ok_or("held carrier")?
            .body,
        full.held.draft.body
    );
    store.mark_push_attempted(&full.last.draft.push_id).await?;
    store
        .settle_push_record(&full.last.draft.push_id, accepted(), Utc::now())
        .await?;
    ensure!(
        store
            .read_local_mail(&mail_id(&full.last)?)
            .await?
            .ok_or("terminal mail")?
            .compacted
    );
    ensure!(matches!(
        store.prepare_local_message(&rejected).await?,
        LocalMessageAdmission::Pending(_)
    ));
    ensure!(
        store
            .get_push_record(&rejected.draft.push_id)
            .await?
            .is_some()
    );
    observer.close().await?;
    store.close().await?;
    Ok(())
}
