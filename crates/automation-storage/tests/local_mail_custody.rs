//! MS-2/MS-3/MS-7: identity, one outcome authority, and carrier history on real SQLite.
use automation_storage::{
    AutomationStore, LocalMessageAdmission, LocalMessageRequest, StorageError,
};
use chrono::{Duration, Utc};
use collaboration_mail_protocol::{MailRefusal, MailStatus};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MessageDelivery, PushDeliveryState, PushOrigin,
};
use sqlx::Connection;
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
fn held() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "target not running".into(),
        },
        reachability: None,
        client: None,
    }
}

#[tokio::test]
async fn terminal_compact_identity_replays_after_carrier_pruning_and_conflicts_before_effect()
-> TestResult {
    let fixture = Fixture::new();
    let now = Utc::now();
    let mut store = AutomationStore::open(&fixture.path).await?;
    let mut input = request(now - Duration::days(80))?;
    let id = mail_id(&input)?;
    store.prepare_local_message(&input).await?;
    store.mark_push_attempted(&input.draft.push_id).await?;
    store
        .settle_push_record(&input.draft.push_id, accepted(), now - Duration::days(31))
        .await?;
    let original = store.read_local_mail(&id).await?.ok_or("custody missing")?;
    ensure!(matches!(original.status, MailStatus::Delivered { .. }));
    ensure!(original.compacted);
    let first_answer = original.result.ok_or("recorded result missing")?;
    ensure_eq!(store.prune_push_records(now, 500).await?, 1);
    ensure!(store.get_push_record(&input.draft.push_id).await?.is_none());
    store.close().await?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    input.draft.created_at = now;
    input.target_identity = "renamed target label".into();
    let replay = store.prepare_local_message(&input).await?;
    let LocalMessageAdmission::Replayed(answer) = replay else {
        return Err("terminal replay prepared a second effect".into());
    };
    ensure_eq!(
        serde_json::to_value(answer)?,
        serde_json::to_value(first_answer)?
    );
    input.draft.body = Some("different content".into());
    ensure!(
        matches!(store.prepare_local_message(&input).await, Err(StorageError::MailRefused(MailRefusal::ItemConflict { existing })) if existing == original.digest)
    );
    ensure!(store.get_push_record(&input.draft.push_id).await?.is_none());
    input.draft.push_id = uuid::Uuid::now_v7().to_string().try_into()?;
    ensure!(matches!(
        store.prepare_local_message(&input).await?,
        LocalMessageAdmission::Pending(_)
    ));
    ensure!(store.get_push_record(&input.draft.push_id).await?.is_some());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn pending_and_held_survive_day_thirty_restart_and_full_content_delivery_until_day_seventy()
-> TestResult {
    for is_held in [false, true] {
        let fixture = Fixture::new();
        let created = Utc::now() - Duration::days(80);
        let input = request(created)?;
        let id = mail_id(&input)?;
        let mut store = AutomationStore::open(&fixture.path).await?;
        store.prepare_local_message(&input).await?;
        if is_held {
            store
                .hold_unsent_direct_message(&input.draft.push_id, held())
                .await?;
        }
        ensure_eq!(
            store
                .prune_push_records(created + Duration::days(39), 500)
                .await?,
            0
        );
        store.close().await?;
        let mut store = AutomationStore::open(&fixture.path).await?;
        ensure!(
            store
                .direct_message_recovery_targets()
                .await?
                .contains(&input.draft.target)
        );
        let inventory = store
            .list_unsent_direct_messages(&input.draft.target)
            .await?;
        ensure_eq!(inventory.len(), 1);
        let pending = inventory.first().ok_or("inventory lost held mail")?;
        ensure_eq!(&pending.body, &input.draft.body);
        ensure!(pending.settled_at.is_none());
        let attempt = store.mark_push_attempted(&input.draft.push_id).await?;
        ensure!(attempt.settled_at.is_none());
        let outcome_at = created + Duration::days(40);
        let delivered = store
            .settle_push_record(&input.draft.push_id, accepted(), outcome_at)
            .await?;
        ensure_eq!(delivered.delivery_state, PushDeliveryState::Delivered);
        ensure_eq!(delivered.settled_at, Some(outcome_at));
        ensure!(
            store
                .read_local_mail(&id)
                .await?
                .ok_or("mail missing")?
                .compacted
        );
        let replay = store.prepare_local_message(&input).await?;
        ensure!(matches!(replay, LocalMessageAdmission::Replayed(_)));
        ensure!(
            store
                .reject_unsent_direct_message(
                    &input.draft.push_id,
                    held(),
                    outcome_at + Duration::days(1)
                )
                .await
                .is_err()
        );
        ensure_eq!(
            store
                .get_push_record(&input.draft.push_id)
                .await?
                .ok_or("push missing")?
                .settled_at,
            Some(outcome_at)
        );
        ensure_eq!(
            store
                .prune_push_records(created + Duration::days(70), 500)
                .await?,
            0
        );
        store.close().await?;
        let mut store = AutomationStore::open(&fixture.path).await?;
        ensure_eq!(
            store
                .get_push_record(&input.draft.push_id)
                .await?
                .ok_or("history expired before deadline")?
                .body,
            input.draft.body
        );
        ensure_eq!(
            store
                .prune_push_records(created + Duration::days(70) + Duration::nanoseconds(1), 500)
                .await?,
            1
        );
        ensure!(matches!(
            store.prepare_local_message(&input).await?,
            LocalMessageAdmission::Replayed(_)
        ));
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn interrupted_attempt_settles_custody_to_uncertain_once_without_recoverable_input()
-> TestResult {
    let fixture = Fixture::new();
    let created = Utc::now() - Duration::days(80);
    let input = request(created)?;
    let id = mail_id(&input)?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    store.mark_push_attempted(&input.draft.push_id).await?;
    ensure_eq!(
        store
            .prune_push_records(created + Duration::days(39), 500)
            .await?,
        0
    );
    store.close().await?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    let recovered_at = created + Duration::days(40);
    ensure_eq!(store.settle_interrupted_pushes(recovered_at).await?, 1);
    let mail = store.read_local_mail(&id).await?.ok_or("mail missing")?;
    ensure!(matches!(mail.status, MailStatus::Uncertain {}));
    ensure!(mail.compacted);
    ensure_eq!(mail.terminal_at, Some(recovered_at));
    ensure!(
        store
            .list_unsent_direct_messages(&input.draft.target)
            .await?
            .is_empty()
    );
    ensure_eq!(
        store
            .settle_interrupted_pushes(recovered_at + Duration::days(5))
            .await?,
        0
    );
    ensure_eq!(
        store
            .get_push_record(&input.draft.push_id)
            .await?
            .ok_or("push missing")?
            .settled_at,
        Some(recovered_at)
    );
    ensure_eq!(
        store
            .prune_push_records(created + Duration::days(70), 500)
            .await?,
        0
    );
    ensure_eq!(
        store
            .prune_push_records(created + Duration::days(71), 500)
            .await?,
        1
    );
    ensure!(
        matches!(store.prepare_local_message(&input).await?, LocalMessageAdmission::Replayed(answer) if matches!(answer.receipt.outcome, DeliveryOutcome::Unknown))
    );
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn rejected_custody_update_rolls_back_the_carrier_and_admission_collision_rolls_back_mail()
-> TestResult {
    let fixture = Fixture::new();
    let input = request(Utc::now())?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    let mut observer = fixture.observer().await?;
    sqlx::query("CREATE TRIGGER reject_mail_update BEFORE UPDATE ON mail_items BEGIN SELECT RAISE(FAIL,'test custody write failure'); END").execute(&mut observer).await?;
    ensure!(
        store
            .hold_unsent_direct_message(&input.draft.push_id, held())
            .await
            .is_err()
    );
    ensure_eq!(
        store
            .get_push_record(&input.draft.push_id)
            .await?
            .ok_or("push missing")?
            .delivery_state,
        PushDeliveryState::Pending
    );
    sqlx::query("DROP TRIGGER reject_mail_update")
        .execute(&mut observer)
        .await?;
    let another = request(Utc::now())?;
    let id = mail_id(&another)?;
    store.insert_push_record(another.draft.clone()).await?;
    ensure!(store.prepare_local_message(&another).await.is_err());
    ensure!(store.read_local_mail(&id).await?.is_none());
    observer.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn same_identity_checks_every_delivery_operand_and_pending_replay_prepares_nothing()
-> TestResult {
    let fixture = Fixture::new();
    let input = request(Utc::now())?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    ensure!(
        matches!(store.prepare_local_message(&input).await?, LocalMessageAdmission::Pending(id) if id == input.draft.push_id)
    );
    for operand in ["body", "target", "sender", "mode", "guard"] {
        let mut changed = LocalMessageRequest {
            draft: input.draft.clone(),
            target_identity: input.target_identity.clone(),
        };
        match operand {
            "body" => changed.draft.body = Some("changed body".into()),
            "target" => changed.draft.target.session_id = "changed-target".to_owned().try_into()?,
            "sender" => changed.draft.origin = PushOrigin::Session(changed.draft.target.clone()),
            "mode" => changed.draft.mode = Some(MessageDelivery::Queue),
            "guard" => {
                changed.draft.guard = Some(serde_json::from_value(
                    serde_json::json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
                )?)
            }
            _ => return Err("unknown fixture operand".into()),
        }
        ensure!(matches!(
            store.prepare_local_message(&changed).await,
            Err(StorageError::MailRefused(MailRefusal::ItemConflict { .. }))
        ));
    }
    let mut observer = fixture.observer().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM router_pushes")
        .fetch_one(&mut observer)
        .await?;
    ensure_eq!(count, 1);
    observer.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn carrier_retention_survives_absent_custody_and_corrupt_retention_is_refused() -> TestResult
{
    let fixture = Fixture::new();
    let created = Utc::now() - Duration::days(80);
    let input = request(created)?;
    let id = mail_id(&input)?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    store.mark_push_attempted(&input.draft.push_id).await?;
    let outcome = created + Duration::days(40);
    store
        .settle_push_record(&input.draft.push_id, accepted(), outcome)
        .await?;
    let mut observer = fixture.observer().await?;
    // Named stand-in for M4 incoming-row retirement: the class must live on
    // the carrier, independently of any custody row's presence or contents.
    sqlx::query("DELETE FROM mail_items WHERE item_id=?")
        .bind(id.as_str())
        .execute(&mut observer)
        .await?;
    observer.close().await?;
    store.close().await?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    ensure_eq!(
        store
            .prune_push_records(created + Duration::days(70), 500)
            .await?,
        0
    );
    ensure!(store.get_push_record(&input.draft.push_id).await?.is_some());
    let mut observer = fixture.observer().await?;
    sqlx::query("UPDATE router_pushes SET retention='badRetention' WHERE push_id=?")
        .bind(input.draft.push_id.as_str())
        .execute(&mut observer)
        .await?;
    ensure!(store.get_push_record(&input.draft.push_id).await.is_err());
    ensure!(
        store
            .prune_push_records(created + Duration::days(71), 500)
            .await
            .is_err()
    );
    sqlx::query("UPDATE router_pushes SET retention='mailTerminalAge' WHERE push_id=?")
        .bind(input.draft.push_id.as_str())
        .execute(&mut observer)
        .await?;
    ensure_eq!(
        store
            .prune_push_records(created + Duration::days(71), 500)
            .await?,
        1
    );
    observer.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn stored_mail_corruption_refuses_replay_before_a_carrier_write() -> TestResult {
    let fixture = Fixture::new();
    let input = request(Utc::now())?;
    let id = mail_id(&input)?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    let mut observer = fixture.observer().await?;
    sqlx::query("UPDATE mail_items SET status_json='{\"kind\":\"unknownStatus\"}' WHERE item_id=?")
        .bind(id.as_str())
        .execute(&mut observer)
        .await?;
    ensure!(matches!(
        store.prepare_local_message(&input).await,
        Err(StorageError::InvalidMail { field: "status" })
    ));
    ensure_eq!(
        store
            .get_push_record(&input.draft.push_id)
            .await?
            .ok_or("push missing")?
            .delivery_state,
        PushDeliveryState::Pending
    );
    observer.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn independent_connections_admit_one_carrier_for_the_same_identity() -> TestResult {
    let fixture = Fixture::new();
    let input = request(Utc::now())?;
    let mut first = AutomationStore::open(&fixture.path).await?;
    let mut second = AutomationStore::open(&fixture.path).await?;
    let (left, right) = tokio::join!(
        first.prepare_local_message(&input),
        second.prepare_local_message(&input)
    );
    ensure!(matches!(left?, LocalMessageAdmission::Pending(_)));
    ensure!(matches!(right?, LocalMessageAdmission::Pending(_)));
    let mut observer = fixture.observer().await?;
    let pushes: i64 = sqlx::query_scalar("SELECT count(*) FROM router_pushes")
        .fetch_one(&mut observer)
        .await?;
    let mails: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_items")
        .fetch_one(&mut observer)
        .await?;
    ensure_eq!(pushes, 1);
    ensure_eq!(mails, 1);
    observer.close().await?;
    first.close().await?;
    second.close().await?;
    Ok(())
}

#[tokio::test]
async fn nonterminal_mail_clock_is_refused_before_read_prune_or_attempt() -> TestResult {
    let fixture = Fixture::new();
    let created = Utc::now() - Duration::days(80);
    let input = request(created)?;
    let mut store = AutomationStore::open(&fixture.path).await?;
    store.prepare_local_message(&input).await?;
    let mut observer = fixture.observer().await?;
    // Pending+rejected receipt is valid legacy CreationAge retry evidence,
    // but MailTerminalAge may never use a nonterminal clock as its outcome.
    let receipt=serde_json::json!({"outcome":{"kind":"rejected","reason":"noRoute","nextAction":"retryLater"},"reachability":null,"client":null}).to_string();
    sqlx::query("UPDATE router_pushes SET last_outcome_json=?,settled_at=? WHERE push_id=?")
        .bind(&receipt)
        .bind((created + Duration::days(1)).to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
        .bind(input.draft.push_id.as_str())
        .execute(&mut observer)
        .await?;
    ensure!(store.get_push_record(&input.draft.push_id).await.is_err());
    ensure!(
        store
            .prune_push_records(created + Duration::days(40), 500)
            .await
            .is_err()
    );
    ensure!(
        store
            .mark_push_attempted(&input.draft.push_id)
            .await
            .is_err()
    );
    let retained:i64=sqlx::query_scalar("SELECT count(*) FROM router_pushes WHERE push_id=? AND delivery_state='pending' AND last_outcome_json=? AND settled_at IS NOT NULL").bind(input.draft.push_id.as_str()).bind(receipt).fetch_one(&mut observer).await?;
    ensure_eq!(retained, 1);
    observer.close().await?;
    store.close().await?;
    Ok(())
}
