//! Wake admission protects an occupying session slot and the bounded timer budget.
use agent_automation::{ExpiryRule, OperationId, TimingRule};
use automation_storage::{AutomationStore, WakeAction, WakeCreate, WakeMutation};
use collaboration_protocol::{SavedMessage, WakeCreator, WakeRefusal};
use serde_json::json;
#[path = "support/wake_push_draft.rs"]
mod wake_push_draft;

macro_rules! ensure {
    ($condition:expr $(,)?) => { ensure!($condition, "assertion failed: {}", stringify!($condition)); };
    ($condition:expr, $($context:tt)+) => {
        if !$condition { return Err(format!($($context)+).into()); }
    };
}
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => { ensure_eq!($left, $right, "values differ"); };
    ($left:expr, $right:expr, $($context:tt)+) => {{
        let left = &$left; let right = &$right;
        if left != right { return Err(format!("{}: left={left:?}, right={right:?}", format_args!($($context)+)).into()); }
    }};
}
macro_rules! ensure_ne {
    ($left:expr, $right:expr $(,)?) => {
        ensure!($left != $right, "values must differ");
    };
}
type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error>>;
const NOW: i64 = 1_760_616_000_000;

fn request(timing: TimingRule, expiry: ExpiryRule) -> TestResult<WakeCreate<SavedMessage>> {
    Ok(WakeCreate {
        requested_by: collaboration_protocol::WakeCreator::Owner {},
        replace: false,
        operation_id: OperationId::generate(),
        message: serde_json::from_value(json!({
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"wake-target"},
            "content":{"kind":"humanUser","text":"A bounded reminder"},"delivery":"auto"
        }))?,
        timing,
        expiry,
        now_ms: NOW,
    })
}

#[tokio::test]
async fn active_and_paused_wakes_both_occupy_the_target_slot() -> TestResult {
    for paused in [false, true] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        let first = store
            .create_wakeup(&request(
                TimingRule::After { seconds: 600 },
                ExpiryRule::None,
            )?)
            .await?;
        if paused {
            store
                .mutate_wakeup::<SavedMessage>(&WakeMutation {
                    operation_id: OperationId::generate(),
                    wakeup_id: first.definition.wakeup_id,
                    action: WakeAction::Pause,
                    now_ms: NOW + 1,
                })
                .await?;
        }
        let second = store
            .create_wakeup(&request(
                TimingRule::After { seconds: 900 },
                ExpiryRule::None,
            )?)
            .await;
        ensure!(
            second.is_err(),
            "accepted second wake in occupied slot; paused={paused}"
        );
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn wake_admission_enforces_due_lifetime_expiry_and_interval_limits() -> TestResult {
    let at = chrono::DateTime::from_timestamp_millis(NOW + 86_401_000).ok_or("test instant")?;
    for (timing, expiry, label) in [
        (TimingRule::At { at }, ExpiryRule::None, "at over 24h"),
        (
            TimingRule::After { seconds: 86_401 },
            ExpiryRule::None,
            "after over 24h",
        ),
        (
            TimingRule::Interval { seconds: 300 },
            ExpiryRule::None,
            "repeat without expiry",
        ),
        (
            TimingRule::Interval { seconds: 300 },
            ExpiryRule::After { seconds: 604_801 },
            "repeat over 7d",
        ),
        (
            TimingRule::Interval { seconds: 299 },
            ExpiryRule::After { seconds: 3600 },
            "interval under 5m",
        ),
    ] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        ensure!(
            store
                .create_wakeup(&request(timing, expiry)?)
                .await
                .is_err(),
            "accepted {label}"
        );
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn cron_limit_uses_the_smallest_occurrence_gap_inside_its_lifetime() -> TestResult {
    for expression in ["* * * * *", "0,4,10 * * * *"] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        let input = request(
            TimingRule::Cron {
                expression: expression.into(),
                timezone: "UTC".into(),
            },
            ExpiryRule::After { seconds: 7200 },
        )?;
        ensure!(
            store.create_wakeup(&input).await.is_err(),
            "accepted too frequent cron: {expression}"
        );
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn only_creator_target_or_owner_can_replace_and_replay_never_replaces_twice() -> TestResult {
    let base = request(TimingRule::After { seconds: 600 }, ExpiryRule::None)?;
    let target = base.message.target.clone();
    let mut creator = target.clone();
    creator.session_id = "wake-creator".to_owned().try_into()?;
    let mut outsider = target.clone();
    outsider.session_id = "unrelated-session".to_owned().try_into()?;
    for (requester, allowed) in [
        (WakeCreator::Session(creator.clone()), true),
        (WakeCreator::Session(target.clone()), true),
        (WakeCreator::Owner {}, true),
        (WakeCreator::Session(outsider), false),
        (WakeCreator::Router {}, false),
    ] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        let mut original = request(TimingRule::After { seconds: 600 }, ExpiryRule::None)?;
        original.requested_by = WakeCreator::Session(creator.clone());
        let first = store.create_wakeup(&original).await?;
        let mut replacement = request(TimingRule::After { seconds: 900 }, ExpiryRule::None)?;
        replacement.requested_by = requester.clone();
        replacement.replace = true;
        replacement.now_ms += 1;
        let result = store.create_wakeup(&replacement).await;
        if !allowed {
            ensure!(
                matches!(result, Err(automation_storage::StorageError::WakeRefused(WakeRefusal::WakeExists { wakeup, creator: actual })) if wakeup == first.definition.wakeup_id && actual == original.requested_by)
            );
            ensure_eq!(
                store
                    .read_wakeup::<SavedMessage>(&first.definition.wakeup_id)
                    .await?
                    .state,
                agent_automation::WakeState::Active
            );
        } else {
            let second = result?;
            ensure_ne!(first.definition.wakeup_id, second.definition.wakeup_id);
            ensure_eq!(second.definition.created_by, requester);
            let previous = store
                .read_wakeup::<SavedMessage>(&first.definition.wakeup_id)
                .await?;
            ensure_eq!(previous.state, agent_automation::WakeState::Cancelled);
            ensure!(
                store
                    .wake_transitions_after(
                        &first.definition.wakeup_id,
                        first.latest_event_sequence
                    )
                    .await?
                    .iter()
                    .any(|event| event.kind == "replaced")
            );
            let mut third_request = request(TimingRule::After { seconds: 1200 }, ExpiryRule::None)?;
            third_request.replace = true;
            let third = store.create_wakeup(&third_request).await?;
            replacement.now_ms += 1000;
            let replay = store.create_wakeup(&replacement).await?;
            ensure_eq!(
                serde_json::to_value(replay)?,
                serde_json::to_value(&second)?,
                "replacement replay changed its first answer"
            );
            ensure_eq!(
                store
                    .read_wakeup::<SavedMessage>(&third.definition.wakeup_id)
                    .await?
                    .state,
                agent_automation::WakeState::Active
            );
            replacement.requested_by = WakeCreator::Router {};
            ensure!(
                matches!(
                    store.create_wakeup(&replacement).await,
                    Err(automation_storage::StorageError::OperationConflict)
                ),
                "canonical request omitted requestedBy"
            );
        }
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn wake_limit_boundaries_and_sparse_cron_are_admitted() -> TestResult {
    for (timing, expiry) in [
        (TimingRule::After { seconds: 86_400 }, ExpiryRule::None),
        (
            TimingRule::At {
                at: chrono::DateTime::from_timestamp_millis(NOW + 86_400_000).ok_or("boundary")?,
            },
            ExpiryRule::None,
        ),
        (
            TimingRule::Interval { seconds: 300 },
            ExpiryRule::After { seconds: 604_800 },
        ),
        (
            TimingRule::Cron {
                expression: "*/5 * * * *".into(),
                timezone: "UTC".into(),
            },
            ExpiryRule::After { seconds: 604_800 },
        ),
        // Close pairs outside the wake's actual lifetime are irrelevant.
        (
            TimingRule::Cron {
                expression: "0,1 0 * * *".into(),
                timezone: "UTC".into(),
            },
            ExpiryRule::After { seconds: 300 },
        ),
    ] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        let record = store.create_wakeup(&request(timing, expiry)?).await?;
        ensure_eq!(record.state, agent_automation::WakeState::Active);
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn one_shot_due_at_24_hours_has_a_next_due_and_fires_before_its_default_end() -> TestResult {
    let due = NOW + 86_400_000;
    for timing in [
        TimingRule::After { seconds: 86_400 },
        TimingRule::At {
            at: chrono::DateTime::from_timestamp_millis(due).ok_or("due instant")?,
        },
    ] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        let first = store
            .create_wakeup(&request(timing, ExpiryRule::None)?)
            .await?;
        let id = first.definition.wakeup_id.clone();
        ensure_eq!(
            first.next_due_at_ms,
            Some(due),
            "accepted one-shot lost its firing"
        );
        ensure_eq!(first.definition.expires_at_ms, Some(NOW + 172_800_000));
        ensure!(store.due_wakeup_ids(due - 1, 100).await?.is_empty());
        ensure_eq!(store.due_wakeup_ids(due, 100).await?, vec![id.clone()]);
        let fired = store
            .evaluate_wakeup::<SavedMessage>(&id, due, wake_push_draft::build_test_wake_push_draft)
            .await?;
        match fired {
            automation_storage::WakeEvaluation::Fired { wake, fire, .. } => {
                ensure_eq!(fire.due_at_ms, due);
                ensure_eq!(fire.fired_at_ms, due);
                ensure!(wake.first_fire.is_some());
                ensure!(wake.pending_delivery_id.is_some());
                ensure!(wake.next_due_at_ms.is_none());
            }
            _ => return Err("one-shot expired instead of firing".into()),
        }
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn paused_one_shot_keeps_a_finite_lifetime_when_expiry_is_omitted() -> TestResult {
    // U66 / MS-10: every wake has a maximum lifetime, including paused one-shots.
    let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
    let wake = store
        .create_wakeup(&request(
            TimingRule::After { seconds: 600 },
            ExpiryRule::None,
        )?)
        .await?;
    let paused = store
        .mutate_wakeup::<SavedMessage>(&WakeMutation {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
            action: WakeAction::Pause,
            now_ms: NOW + 1,
        })
        .await?;
    ensure_eq!(paused.wake.state, agent_automation::WakeState::Paused);
    ensure!(
        paused.wake.definition.expires_at_ms.is_some(),
        "paused one-shot has no lifetime end and keeps its slot forever"
    );
    // PD4 §7 at 723ad984: expiry is due + 24h, including while paused.
    ensure_eq!(paused.wake.definition.expires_at_ms, Some(NOW + 87_000_000));
    ensure!(
        store
            .due_wakeup_ids(NOW + 86_400_000, 100)
            .await?
            .is_empty()
    );
    let later = NOW + 87_000_000;
    ensure_eq!(
        store.due_wakeup_ids(later, 100).await?,
        vec![wake.definition.wakeup_id.clone()]
    );
    let expired = store
        .evaluate_wakeup::<SavedMessage>(&wake.definition.wakeup_id, later, |_, _| {
            Err(automation_storage::StorageError::InvalidRecord)
        })
        .await?;
    ensure!(matches!(
        expired,
        automation_storage::WakeEvaluation::Expired(_)
    ));
    let mut next = request(TimingRule::After { seconds: 600 }, ExpiryRule::None)?;
    next.now_ms = later;
    let next = store.create_wakeup(&next).await?;
    ensure_ne!(next.definition.wakeup_id, wake.definition.wakeup_id);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn one_shot_explicit_expiry_keeps_early_and_exact_limits_and_refuses_later() -> TestResult {
    // PD4 §7 at 723ad984: due is +600s; its exact end cap is +87000s.
    for expiry in [
        ExpiryRule::After { seconds: 87_001 },
        ExpiryRule::At {
            at: chrono::DateTime::from_timestamp_millis(NOW + 87_001_000).ok_or("late boundary")?,
        },
    ] {
        let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
        ensure!(
            matches!(
                store
                    .create_wakeup(&request(TimingRule::After { seconds: 600 }, expiry)?)
                    .await,
                Err(automation_storage::StorageError::WakeRefused(
                    WakeRefusal::LifetimeTooLong { max: 86_400 }
                ))
            ),
            "late explicit one-shot end was accepted"
        );
        store.close().await?;
    }
    for seconds in [3600, 86_400, 87_000] {
        for expiry in [
            ExpiryRule::After { seconds },
            ExpiryRule::At {
                at: chrono::DateTime::from_timestamp_millis(NOW + i64::from(seconds) * 1000)
                    .ok_or("valid boundary")?,
            },
        ] {
            let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
            let wake = store
                .create_wakeup(&request(TimingRule::After { seconds: 600 }, expiry)?)
                .await?;
            ensure_eq!(
                wake.definition.expires_at_ms,
                Some(NOW + i64::from(seconds) * 1000)
            );
            store.close().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn one_shot_end_survives_resume_restart_and_delayed_creation_replay() -> TestResult {
    let path = std::env::temp_dir().join(format!(
        "m2c-wake-end-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut store = AutomationStore::open(&path).await?;
    let mut input = request(TimingRule::After { seconds: 600 }, ExpiryRule::None)?;
    let first = store.create_wakeup(&input).await?;
    let id = first.definition.wakeup_id.clone();
    let end = NOW + 87_000_000;
    for (action, now_ms) in [
        (WakeAction::Pause, NOW + 1000),
        (WakeAction::Resume, NOW + 2000),
        (WakeAction::Pause, NOW + 3000),
    ] {
        let changed = store
            .mutate_wakeup::<SavedMessage>(&WakeMutation {
                operation_id: OperationId::generate(),
                wakeup_id: id.clone(),
                action,
                now_ms,
            })
            .await?;
        ensure_eq!(
            changed.wake.definition.expires_at_ms,
            Some(end),
            "resume moved the original one-shot end"
        );
    }
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    input.now_ms = NOW + 2 * 86_400_000;
    let replay = store.create_wakeup(&input).await?;
    ensure_eq!(
        serde_json::to_value(replay)?,
        serde_json::to_value(&first)?,
        "delayed replay changed the first answer"
    );
    let current = store.read_wakeup::<SavedMessage>(&id).await?;
    ensure_eq!(current.state, agent_automation::WakeState::Paused);
    ensure_eq!(current.definition.expires_at_ms, Some(end));
    ensure_eq!(
        store.due_wakeup_ids(input.now_ms, 100).await?,
        vec![id.clone()]
    );
    let expired = store
        .evaluate_wakeup::<SavedMessage>(&id, input.now_ms, |_, _| {
            Err(automation_storage::StorageError::InvalidRecord)
        })
        .await?;
    ensure!(matches!(
        expired,
        automation_storage::WakeEvaluation::Expired(_)
    ));
    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[tokio::test]
async fn expiry_precedes_a_due_time_at_the_effective_end() -> TestResult {
    // PD4 §7 at 723ad984 keeps an earlier explicit expiry. At that exclusive
    // end expiry still wins; the default end is now a day after this due time.
    let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
    let first = store
        .create_wakeup(&request(
            TimingRule::After { seconds: 86_400 },
            ExpiryRule::After { seconds: 86_400 },
        )?)
        .await?;
    let id = first.definition.wakeup_id;
    let end = NOW + 86_400_000;
    ensure!(store.due_wakeup_ids(end - 1, 100).await?.is_empty());
    let before = store
        .evaluate_wakeup::<SavedMessage>(&id, end - 1, |_, _| {
            Err(automation_storage::StorageError::InvalidRecord)
        })
        .await?;
    ensure!(
        matches!(before, automation_storage::WakeEvaluation::Unchanged(ref record) if record.state == agent_automation::WakeState::Active)
    );
    ensure_eq!(store.due_wakeup_ids(end, 100).await?, vec![id.clone()]);
    let at = store
        .evaluate_wakeup::<SavedMessage>(&id, end, |_, _| {
            Err(automation_storage::StorageError::InvalidRecord)
        })
        .await?;
    ensure!(matches!(at, automation_storage::WakeEvaluation::Expired(_)));
    let after = store
        .evaluate_wakeup::<SavedMessage>(&id, end + 1, |_, _| {
            Err(automation_storage::StorageError::InvalidRecord)
        })
        .await?;
    ensure!(
        matches!(after, automation_storage::WakeEvaluation::Unchanged(ref record) if record.state == agent_automation::WakeState::Expired)
    );
    let current = store.read_wakeup::<SavedMessage>(&id).await?;
    ensure!(current.first_fire.is_none());
    ensure!(current.pending_delivery_id.is_none());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_replacement_preserves_the_occupant_and_creates_no_receipt_or_event() -> TestResult
{
    let mut store = AutomationStore::open(std::path::Path::new(":memory:")).await?;
    let first = store
        .create_wakeup(&request(
            TimingRule::After { seconds: 600 },
            ExpiryRule::None,
        )?)
        .await?;
    for (timing, expiry) in [
        (TimingRule::After { seconds: 86_401 }, ExpiryRule::None),
        (
            TimingRule::After { seconds: 600 },
            ExpiryRule::After { seconds: 87_001 },
        ),
        (TimingRule::Interval { seconds: 300 }, ExpiryRule::None),
        (
            TimingRule::Interval { seconds: 299 },
            ExpiryRule::After { seconds: 3600 },
        ),
    ] {
        let mut input = request(timing, expiry)?;
        input.replace = true;
        ensure!(matches!(
            store.create_wakeup(&input).await,
            Err(automation_storage::StorageError::WakeRefused(_))
        ));
        let current = store
            .read_wakeup::<SavedMessage>(&first.definition.wakeup_id)
            .await?;
        ensure_eq!(
            serde_json::to_value(current)?,
            serde_json::to_value(&first)?
        );
        ensure!(matches!(
            store.read_operation(&input.operation_id).await,
            Err(automation_storage::StorageError::OperationNotFound)
        ));
        ensure!(
            store
                .wake_transitions_after(&first.definition.wakeup_id, first.latest_event_sequence)
                .await?
                .is_empty()
        );
    }
    store.close().await?;
    Ok(())
}
