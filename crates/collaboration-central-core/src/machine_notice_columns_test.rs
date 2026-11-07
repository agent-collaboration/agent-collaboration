use super::{NoticeColumns, decode_notice};
use board_sync_protocol::BoardNotice;
use fleet_protocol::Epoch;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn every_control_variant_roundtrips_through_its_columns() -> TestResult {
    // Arrange: the three owner-approved project control variants.
    let project = "019f0000-0000-7000-8000-000000000901"
        .to_owned()
        .try_into()?;
    let notices = [
        BoardNotice::ProjectListChanged {},
        BoardNotice::LeaseGranted {
            project,
            epoch: Epoch::new(3)?,
        },
        BoardNotice::LeaseSuperseded {
            project: "019f0000-0000-7000-8000-000000000901"
                .to_owned()
                .try_into()?,
            epoch: Epoch::new(2)?,
        },
    ];
    for expected in notices {
        if matches!(expected, BoardNotice::ProjectListChanged {}) {
            assert_eq!(
                serde_json::to_value(&expected)?["kind"],
                "projectListChanged"
            );
        }
        // Act: the real storage codec is the boundary under test.
        let row = NoticeColumns::from_notice(expected.clone());
        let actual = decode_notice(Some(row.kind.into()), row.project, row.epoch)?;
        // Assert: a lost epoch, wrong kind or changed identity changes the consumer's notice.
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
fn retired_notice_kinds_are_refused_instead_of_coerced() {
    for kind in [
        "mail_waiting_items",
        "mail_waiting_controls",
        "fleet_feed_advanced",
        "board_archiving",
    ] {
        assert!(matches!(decode_notice(Some(kind.into()),None,None),
            Err(durable_steps::StoreError::Corrupt {field,..}) if field=="fleet.machine_control_project_notices.kind"));
    }
    for notice in [
        serde_json::json!({"kind":"mailWaiting","value":{"stream":"items"}}),
        serde_json::json!({"kind":"mailWaiting","value":{"stream":"controls"}}),
        serde_json::json!({"kind":"fleetFeedAdvanced","value":{"through":12}}),
        serde_json::json!({"kind":"boardArchiving","value":{"board":"019f0000-0000-7000-8000-000000000902"}}),
    ] {
        assert!(serde_json::from_value::<BoardNotice>(notice).is_err());
    }
}

#[test]
fn unknown_kinds_and_mismatched_variant_columns_are_refused() {
    let project = "019f0000-0000-7000-8000-000000000901";
    for (kind, project, epoch) in [
        ("future_notice", None, None),
        ("lease_granted", None, Some("3")),
        ("lease_granted", Some(project), Some("0")),
        ("lease_granted", Some(project), Some("012")),
        ("lease_granted", Some(project), Some("9223372036854775808")),
        (
            "lease_granted",
            Some("550e8400-e29b-41d4-a716-446655440001"),
            Some("3"),
        ),
        ("project_list_changed", Some(project), None),
        ("project_list_changed", None, Some("3")),
        ("mail_waiting_items", None, Some("3")),
    ] {
        assert!(
            decode_notice(
                Some(kind.into()),
                project.map(str::to_owned),
                epoch.map(str::to_owned)
            )
            .is_err(),
            "accepted corrupt {kind} columns"
        );
    }
}
