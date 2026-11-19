//! Acceptance R2/R3 on workerd: the recorded start order of one due batch's
//! units across wakes, automations, probe items and the two notice units, each
//! machine kind earliest first. Writes the visit-order receipt.
use super::super::engine_evidence::append_receipt;
use super::super::engine_v2_support::*;
use super::super::support::*;
use super::engine_probe_support::{probe_id, seed_probe};

#[tokio::test]
async fn one_due_batch_starts_each_kind_earliest_first_then_the_notice_units() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (five, ten) = (start + minutes(5), start + minutes(10));
    // Created latest-first, so start order cannot follow insertion order.
    let late_wake = set_wake(&fixture, 16000, ten, "late wake").await;
    let early_wake = set_wake(&fixture, 16001, five, "early wake").await;
    let late_automation = set_automation_at(&fixture, 16002, ten, "late automation").await;
    let early_automation = set_automation_at(&fixture, 16003, five, "early automation").await;
    let group = probe_id(16010);
    let (late_probe, early_probe) = (probe_id(16011), probe_id(16012));
    seed_probe(&runtime, late_probe, group, Some(ten), "late probe").await;
    seed_probe(&runtime, early_probe, group, Some(five), "early probe").await;

    set_now(&fixture, ten);
    runtime.after_event().await.unwrap();

    let visited: Vec<_> = runtime
        .due_unit_starts()
        .await
        .into_iter()
        .filter(|unit| unit.batch_start_unix_millis == ten.timestamp_millis())
        .collect();
    let order: Vec<(String, String)> = visited
        .iter()
        .map(|unit| (unit.machine_kind.clone(), unit.subject_key.clone()))
        .collect();
    let expected: Vec<(String, String)> = [
        ("wake_timer", early_wake),
        ("wake_timer", late_wake),
        ("automation_timer", early_automation),
        ("automation_timer", late_automation),
        ("engine_probe", early_probe.to_string()),
        ("engine_probe", late_probe.to_string()),
        ("legacy_notice_windows", "all".to_owned()),
        ("legacy_workspace_drains", "all".to_owned()),
    ]
    .into_iter()
    .map(|(kind, key)| (kind.to_owned(), key))
    .collect();
    append_receipt(
        "r2-r3-visit-order.json",
        &serde_json::json!({
            "test": "one_due_batch_starts_each_kind_earliest_first_then_the_notice_units",
            "batchStartUnixMillis": ten.timestamp_millis(),
            "dueTimes": {"early": five.to_rfc3339(), "late": ten.to_rfc3339()},
            "unitStarts": visited,
        })
        .to_string(),
    );
    assert_eq!(order, expected);
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM mail_messages WHERE message_kind IN ('wake','automation')",
            vec![]
        )
        .await,
        4,
        "every visited timer fired once"
    );
    fixture.host.close().await;
}
