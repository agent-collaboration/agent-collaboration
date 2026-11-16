//! V13 on the native host: EngineHealth, the retired list with paging, retry
//! that wakes an idle object, discard through the machine's own transition and
//! admission, each through the real admin route with a real person proof.
// Test-only module: clippy's test allowances (clippy.toml) apply to its helpers.
#![cfg(test)]
use super::engine_admin_support::*;
use super::support::text;
use collaboration_central_core::PausePoint;
use serde_json::json;

const REJECT_AUTOMATION: &str = "CREATE TRIGGER automation_reject BEFORE UPDATE OF evaluated_through_at ON automations BEGIN SELECT RAISE(ABORT,'automation rejected'); END";

/// Corrupt wakes due at the next mark, run through two passes: all retired.
async fn retired_wakes(
    engine: &EngineOrg,
    count: usize,
) -> (Vec<String>, Time, central_storage::SqlValue) {
    let first = next_mark(engine.now());
    let mut wakes = Vec::new();
    let mut original = central_storage::SqlValue::Null;
    for index in 0..count {
        let wake = engine.wake(first, &format!("broken {index}")).await;
        original = engine
            .rows(
                "SELECT target FROM wakes WHERE wake_id=?",
                vec![text(&wake)],
            )
            .await[0][0]
            .clone();
        engine
            .raw(
                "UPDATE wakes SET target='not-json' WHERE wake_id=?",
                vec![text(&wake)],
            )
            .await;
        wakes.push(wake);
    }
    engine.set(first);
    engine.after_event().await;
    engine.set(first + minutes(5));
    engine.after_event().await;
    for wake in &wakes {
        assert_eq!(
            engine.record("wake_timer", wake).await[0][..2],
            [text("retired"), integer(2)]
        );
    }
    (wakes, first, original)
}

#[tokio::test]
async fn health_lists_every_kind_and_the_retired_list_pages_in_attempt_order() {
    let engine = EngineOrg::new().await;
    let (_, first, _) = retired_wakes(&engine, 5).await;
    let failing_due = next_mark(engine.now());
    let failing = engine.wake(failing_due, "failing").await;
    engine
        .raw(
            "UPDATE wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&failing)],
        )
        .await;
    engine.set(failing_due);
    engine.after_event().await;

    let health = engine.health().await;

    assert_eq!(health["result"], "engineHealth");
    let kinds: Vec<_> = health["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            "wake_timer",
            "automation_timer",
            "legacy_notice_windows",
            "legacy_workspace_drains",
            "engine_source"
        ],
        "every registered kind, zeros included"
    );
    let wakes = kind_health(&health, "wake_timer");
    assert_eq!(
        (&wakes["failing"], &wakes["retired"]),
        (&json!(1), &json!(5))
    );
    assert_eq!(wakes["lastFailure"], "corrupt_row");
    let oldest: Time = wakes["oldestFirstFailedAt"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(oldest, first, "the oldest first failure is the first pass");
    for kind in [
        "automation_timer",
        "legacy_notice_windows",
        "legacy_workspace_drains",
        "engine_source",
    ] {
        assert_eq!(
            kind_health(&health, kind),
            &json!({"kind":kind,"failing":0,"retired":0,"oldestFirstFailedAt":null,"lastFailure":null})
        );
    }
    assert_eq!(health["alarmMissing"], false);

    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    let mut sizes = Vec::new();
    loop {
        let page = engine.retired(after.as_deref(), 2).await;
        assert_eq!(page["result"], "engineRetired");
        let items = page["items"].as_array().unwrap();
        sizes.push(items.len());
        for item in items {
            assert_eq!(item["kind"], "wake_timer");
            assert_eq!(item["failureClass"], "corrupt_row");
            assert_eq!(item["count"], 2);
            seen.push(item["attemptId"].as_str().unwrap().to_owned());
        }
        match page["next"].as_str() {
            Some(next) => {
                assert_eq!(
                    Some(next),
                    seen.last().map(String::as_str),
                    "next is the page's last id"
                );
                after = Some(next.to_owned());
            }
            None => break,
        }
    }
    assert_eq!(sizes, [2, 2, 1]);
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(seen, sorted, "ascending attemptId, each row once");
    assert_eq!(seen.len(), 5, "the failing row is not listed");

    // The reads need organization:readAudit.
    let no_audit = engine.person(&["organization:view"], false);
    assert_eq!(engine.health_as(&no_audit).await, refused("notPermitted"));
    assert_eq!(
        engine.retired_as(None, 2, &no_audit).await,
        refused("notPermitted")
    );
    engine.close().await;
}

#[tokio::test]
async fn retry_wakes_an_idle_object_and_refuses_missing_failing_and_unadmitted_changes() {
    let engine = EngineOrg::new().await;
    let (wakes, _, original_target) = retired_wakes(&engine, 3).await;
    assert_eq!(engine.alarm_ms().await, None, "everything else is idle");
    let attempt = engine.attempt_id("wake_timer", &wakes[0]).await;
    engine
        .raw(
            "UPDATE wakes SET target=? WHERE wake_id=?",
            vec![original_target.clone(), text(&wakes[0])],
        )
        .await;

    // Admission first: nothing changes without a second factor or the permission.
    let operation = engine.fresh();
    assert_eq!(
        engine
            .change_as(
                RETRY,
                &operation,
                &attempt,
                &engine.person(&[READ_AUDIT, REPAIR_WORK], false)
            )
            .await,
        refused("secondFactorRequired")
    );
    assert_eq!(
        engine
            .change_as(
                RETRY,
                &engine.fresh(),
                &attempt,
                &engine.person(&[READ_AUDIT], true)
            )
            .await,
        refused("notPermitted")
    );
    assert_eq!(
        engine.record("wake_timer", &wakes[0]).await[0][0],
        text("retired")
    );

    // Retried within 30 s of the last due batch's start: P4 places the alarm at
    // its floor, last start + 30 s. (V13 as ruled: re-published by P4's rule.)
    let last_start = engine.now();
    let answer = engine
        .change_as(RETRY, &operation, &attempt, &engine.repairer())
        .await;

    assert_eq!(answer, json!({"result":"engineRetried"}));
    assert!(
        engine.record("wake_timer", &wakes[0]).await.is_empty(),
        "record gone"
    );
    let floor = last_start + chrono::Duration::seconds(30);
    assert_eq!(
        engine.alarm_ms().await,
        Some(floor.timestamp_millis()),
        "alarm published"
    );
    assert_eq!(
        engine
            .change_as(RETRY, &operation, &attempt, &engine.repairer())
            .await,
        answer,
        "a replay with the same operation ID returns the stored answer"
    );
    assert_eq!(engine.wake_messages(&wakes[0]).await, 0, "not worked yet");
    engine.set(floor);
    engine.after_event().await;
    assert_eq!(
        engine.wake_messages(&wakes[0]).await,
        1,
        "worked at the published alarm"
    );
    assert_eq!(
        engine
            .rows(
                "SELECT state FROM wakes WHERE wake_id=?",
                vec![text(&wakes[0])]
            )
            .await,
        vec![vec![text("done")]]
    );
    assert_eq!(
        engine.retry(&attempt).await,
        refused("notFound"),
        "second retry"
    );

    // Retried more than 30 s after the last batch: P6's after_event on the
    // admin entry starts a due batch, so the retry request itself works the wake.
    let third = engine.attempt_id("wake_timer", &wakes[2]).await;
    engine
        .raw(
            "UPDATE wakes SET target=? WHERE wake_id=?",
            vec![original_target.clone(), text(&wakes[2])],
        )
        .await;
    engine.set(engine.now() + chrono::Duration::seconds(60));
    assert_eq!(
        engine.retry(&third).await,
        json!({"result":"engineRetried"})
    );
    assert_eq!(engine.wake_messages(&wakes[2]).await, 1);
    assert!(engine.record("wake_timer", &wakes[2]).await.is_empty());

    // Two concurrent retries of one retired row: one wins, one finds nothing.
    let other = engine.attempt_id("wake_timer", &wakes[1]).await;
    let (first, second) = tokio::join!(engine.retry(&other), engine.retry(&other));
    let mut answers = [first, second];
    answers.sort_by_key(|answer| answer.to_string());
    assert_eq!(
        answers,
        [refused("notFound"), json!({"result":"engineRetried"})]
    );

    // A failing row is not retired.
    let due = next_mark(engine.now());
    let failing = engine.wake(due, "failing").await;
    engine
        .raw(
            "UPDATE wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&failing)],
        )
        .await;
    engine.set(due);
    engine.after_event().await;
    let failing_attempt = engine.attempt_id("wake_timer", &failing).await;
    assert_eq!(engine.retry(&failing_attempt).await, refused("notRetired"));
    assert_eq!(
        engine.record("wake_timer", &failing).await[0][..2],
        [text("failing"), integer(1)]
    );
    engine.close().await;
}

/// An automation retired by rejected batches; its row stays readable.
async fn retired_automation(engine: &EngineOrg) -> (String, String) {
    let first = next_mark(engine.now());
    let automation = engine.automation(first, "rejected").await;
    engine.raw(REJECT_AUTOMATION, vec![]).await;
    engine.set(first);
    engine.after_event().await;
    engine.set(first + minutes(5));
    engine.after_event().await;
    engine.raw("DROP TRIGGER automation_reject", vec![]).await;
    assert_eq!(
        engine.record("automation_timer", &automation).await[0][..2],
        [text("retired"), integer(2)]
    );
    let attempt = engine.attempt_id("automation_timer", &automation).await;
    (automation, attempt)
}

async fn automation_state(
    engine: &EngineOrg,
    automation: &str,
) -> Vec<Vec<central_storage::SqlValue>> {
    engine
        .rows(
            "SELECT state FROM automations WHERE automation_id=?",
            vec![text(automation)],
        )
        .await
}

#[tokio::test]
async fn discard_runs_the_finishing_transition_and_refuses_unsupported_and_stale_subjects() {
    let engine = EngineOrg::new().await;
    let (automation, attempt) = retired_automation(&engine).await;
    let operation = engine.fresh();
    assert_eq!(
        engine
            .change_as(
                DISCARD,
                &operation,
                &attempt,
                &engine.person(&[READ_AUDIT, REPAIR_WORK], false)
            )
            .await,
        refused("secondFactorRequired")
    );
    assert_eq!(
        engine
            .change_as(
                DISCARD,
                &engine.fresh(),
                &attempt,
                &engine.person(&[READ_AUDIT], true)
            )
            .await,
        refused("notPermitted")
    );

    let answer = engine
        .change_as(DISCARD, &operation, &attempt, &engine.repairer())
        .await;

    assert_eq!(answer, json!({"result":"engineDiscarded"}));
    assert_eq!(
        automation_state(&engine, &automation).await,
        vec![vec![text("disabled")]]
    );
    assert!(
        engine
            .record("automation_timer", &automation)
            .await
            .is_empty()
    );
    assert_eq!(
        engine
            .change_as(DISCARD, &operation, &attempt, &engine.repairer())
            .await,
        answer,
        "a replay returns the stored answer"
    );
    assert_eq!(engine.discard(&attempt).await, refused("notFound"));

    // Not retired.
    let due = next_mark(engine.now());
    let failing = engine.automation(due, "failing").await;
    engine.raw(REJECT_AUTOMATION, vec![]).await;
    engine.set(due);
    engine.after_event().await;
    engine.raw("DROP TRIGGER automation_reject", vec![]).await;
    let failing_attempt = engine.attempt_id("automation_timer", &failing).await;
    assert_eq!(
        engine.discard(&failing_attempt).await,
        refused("notRetired")
    );
    assert_eq!(
        automation_state(&engine, &failing).await,
        vec![vec![text("active")]]
    );

    // A subject changed meanwhile: the discard's reload finds it disabled.
    let (moved, moved_attempt) = retired_automation(&engine).await;
    let runtime = engine.runtime().await;
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineLoad, &moved);
    let change = async {
        held.until_held().await;
        engine
            .raw(
                "UPDATE automations SET state='disabled' WHERE automation_id=?",
                vec![text(&moved)],
            )
            .await;
        held.release();
    };
    let (stale, ()) = tokio::join!(engine.discard(&moved_attempt), change);
    assert_eq!(stale, refused("stale"));
    assert_eq!(
        engine.record("automation_timer", &moved).await[0][..2],
        [text("retired"), integer(2)],
        "nothing changed: the record stays"
    );
    engine.close().await;
}

/// One retired subject with a corrupt stored value, for the F1 / V13 item 3 cases.
enum Corruption {
    AutomationTimingRule,
    AutomationTarget,
    WakeDueAt,
    WakeTarget,
    AutomationLifecycle,
}

async fn retired_with(engine: &EngineOrg, corruption: &Corruption) -> (&'static str, String) {
    let first = next_mark(engine.now());
    let (kind, subject, sql): (&'static str, String, &'static str) = match corruption {
        Corruption::AutomationTimingRule => (
            "automation_timer",
            engine.automation(first, "corrupt timing").await,
            "UPDATE automations SET timing_rule='not-json' WHERE automation_id=?",
        ),
        Corruption::AutomationTarget => (
            "automation_timer",
            engine.automation(first, "corrupt target").await,
            "UPDATE automations SET target='not-json' WHERE automation_id=?",
        ),
        Corruption::WakeDueAt => (
            "wake_timer",
            engine.wake(first, "corrupt due").await,
            "UPDATE wakes SET due_at='not-a-time' WHERE wake_id=?",
        ),
        Corruption::WakeTarget => (
            "wake_timer",
            engine.wake(first, "corrupt target").await,
            "UPDATE wakes SET target='not-json' WHERE wake_id=?",
        ),
        Corruption::AutomationLifecycle => (
            "automation_timer",
            engine.automation(first, "corrupt state").await,
            "UPDATE automations SET state='mangled' WHERE automation_id=?",
        ),
    };
    engine.raw(sql, vec![text(&subject)]).await;
    engine.set(first);
    engine.after_event().await;
    engine.set(first + minutes(5));
    engine.after_event().await;
    assert_eq!(
        engine.record(kind, &subject).await[0][..2],
        [text("retired"), integer(2)]
    );
    (kind, subject)
}

async fn subject_state(
    engine: &EngineOrg,
    kind: &str,
    subject: &str,
) -> Vec<Vec<central_storage::SqlValue>> {
    match kind {
        "wake_timer" => {
            engine
                .rows(
                    "SELECT state FROM wakes WHERE wake_id=?",
                    vec![text(subject)],
                )
                .await
        }
        _ => automation_state(engine, subject).await,
    }
}

#[tokio::test]
async fn discard_finishes_a_retired_subject_whose_payload_will_not_decode() {
    // F1 / V13 item 3: Discard reads only the lifecycle facts, so a corrupt
    // timing_rule, due_at or target does not stop it; no payload repair first.
    for corruption in [
        Corruption::AutomationTimingRule,
        Corruption::WakeDueAt,
        Corruption::AutomationTarget,
        Corruption::WakeTarget,
    ] {
        let engine = EngineOrg::new().await;
        let (kind, subject) = retired_with(&engine, &corruption).await;
        let attempt = engine.attempt_id(kind, &subject).await;

        let answer = engine.discard(&attempt).await;

        assert_eq!(answer, serde_json::json!({"result": "engineDiscarded"}));
        assert_eq!(
            subject_state(&engine, kind, &subject).await,
            vec![vec![text("disabled")]]
        );
        assert!(
            engine.record(kind, &subject).await.is_empty(),
            "the record is gone in the same batch"
        );
        engine.close().await;
    }
}

#[tokio::test]
async fn discarding_a_retired_subject_whose_lifecycle_state_will_not_decode_reports_subject_unreadable()
 {
    let engine = EngineOrg::new().await;
    let (kind, subject) = retired_with(&engine, &Corruption::AutomationLifecycle).await;
    let before = engine
        .rows(
            "SELECT * FROM central_work_attempts WHERE machine_kind='automation_timer' AND subject_key=?",
            vec![text(&subject)],
        )
        .await;
    let attempt = engine.attempt_id(kind, &subject).await;

    let answer = engine.discard(&attempt).await;

    assert_eq!(answer, refused("subjectUnreadable"));
    assert_eq!(
        engine
            .rows(
                "SELECT * FROM central_work_attempts WHERE machine_kind='automation_timer' AND subject_key=?",
                vec![text(&subject)],
            )
            .await,
        before,
        "nothing changed"
    );
    assert_eq!(
        subject_state(&engine, kind, &subject).await,
        vec![vec![text("mangled")]],
        "never coerced"
    );
    engine.close().await;
}
