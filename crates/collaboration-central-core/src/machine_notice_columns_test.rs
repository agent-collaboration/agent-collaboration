use super::{NoticeColumns, decode_notice};
use board_sync_protocol::BoardNotice;
use fleet_sync_protocol::Epoch;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn every_control_variant_roundtrips_through_its_columns() -> TestResult {
    // Arrange: literals follow the four D1 control-notice variants.
    let project = "019f0000-0000-7000-8000-000000000901"
        .to_owned()
        .try_into()?;
    let notices = [
        BoardNotice::FleetFeedAdvanced {
            through: 12_u64.try_into()?,
        },
        BoardNotice::BoardArchiving {
            board: "019f0000-0000-7000-8000-000000000902"
                .to_owned()
                .try_into()?,
        },
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
        // Act: the real storage codec is the boundary under test.
        let row = NoticeColumns::from_notice(expected.clone());
        let actual = decode_notice(
            Some(row.kind.into()),
            row.board,
            row.project,
            row.epoch,
            row.feed_through,
        )?;
        // Assert: a lost epoch, wrong kind or changed identity changes the consumer's notice.
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
fn retired_mail_waiting_rows_are_refused_instead_of_coerced() {
    for kind in ["mail_waiting_items", "mail_waiting_controls"] {
        assert!(
            matches!(decode_notice(Some(kind.into()),None,None,None,None),
            Err(durable_steps::StoreError::Corrupt {field,..}) if field=="fleet.machine_control_notices.kind")
        );
        assert!(serde_json::from_value::<BoardNotice>(serde_json::json!({"kind":"mailWaiting","value":{"stream":if kind=="mail_waiting_items" {"items"} else {"controls"}}})).is_err());
    }
}

#[test]
fn unknown_kinds_and_mismatched_variant_columns_are_refused() {
    for (kind, board, project, epoch, feed) in [
        ("future_notice", None, None, None, None),
        ("lease_granted", None, None, Some("3"), None),
        (
            "lease_granted",
            None,
            Some("019f0000-0000-7000-8000-000000000901"),
            Some("0"),
            None,
        ),
        (
            "board_archiving",
            Some("550e8400-e29b-41d4-a716-446655440001"),
            None,
            None,
            None,
        ),
        ("fleet_feed_advanced", None, None, None, Some("012")),
        (
            "fleet_feed_advanced",
            None,
            None,
            None,
            Some("9223372036854775808"),
        ),
        ("mail_waiting_items", None, None, Some("3"), None),
    ] {
        let actual = decode_notice(
            Some(kind.into()),
            board.map(str::to_owned),
            project.map(str::to_owned),
            epoch.map(str::to_owned),
            feed.map(str::to_owned),
        );
        assert!(actual.is_err(), "accepted corrupt {kind} columns");
    }
}
