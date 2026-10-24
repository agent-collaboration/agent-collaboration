#![allow(clippy::panic_in_result_fn)]
use board_fleet_sync::BoardPieceAnnotation;
use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{
    CommitSequence, Epoch, ReplicatedTransaction, ReplicationPosition, RowChange, SqlValue,
};
use fleet_sync_writer::{CapturedTransaction, PieceAnnotation};
use message_board::RecordTime;
use message_board_storage::CapturedProjectChanges;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn feed(position: i64, status: Option<&str>, time: Option<&str>) -> RowChange {
    let key = SqlValue::Text(format!("019eef00-0000-7000-8000-{position:012}"));
    RowChange::Upsert {
        table: "project_feed".to_owned(),
        key: vec![key.clone()],
        values: vec![
            key,
            SqlValue::Integer(1),
            SqlValue::Integer(position),
            SqlValue::Text("task".to_owned()),
            SqlValue::Text("019eef00-0000-7000-8000-000000000001".to_owned()),
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Null,
            time.map_or(SqlValue::Null, |value| SqlValue::Text(value.to_owned())),
            SqlValue::Text(
                if time.is_some() {
                    "router"
                } else {
                    "legacy_unknown"
                }
                .to_owned(),
            ),
            SqlValue::Text("{}".to_owned()),
            status.map_or(SqlValue::Null, |value| SqlValue::Text(value.to_owned())),
        ],
    }
}

#[test]
fn annotation_uses_the_highest_feed_position_including_unknown_time_and_no_status() -> TestResult {
    let transaction = ReplicatedTransaction::Rows {
        commit_id: CommitSequence::new(900),
        rows: vec![
            feed(4, None, None),
            feed(2, Some("working"), Some("2026-10-18T12:00:00.000000000Z")),
        ],
    };
    let captured = CapturedTransaction {
        position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(900)),
        transaction: &transaction,
    };
    let decoded = CapturedProjectChanges::decode(&captured)?;
    assert_eq!(
        decoded.first_feed_position.map(|position| position.get()),
        Some(2)
    );
    assert_eq!(
        decoded
            .latest_feed
            .as_ref()
            .map(|latest| latest.cursor.position.get()),
        Some(4)
    );
    assert_eq!(
        BoardPieceAnnotation.annotate(&[captured]),
        Some(BoardAnnotation::ProjectSummary {
            status: None,
            latest_record_at: RecordTime::LegacyUnknown
        })
    );
    Ok(())
}

#[test]
fn no_feed_keeps_the_summary_and_malformed_captured_metadata_refuses() -> TestResult {
    let transaction = ReplicatedTransaction::Rows {
        commit_id: CommitSequence::new(901),
        rows: Vec::new(),
    };
    let captured = CapturedTransaction {
        position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(901)),
        transaction: &transaction,
    };
    assert_eq!(BoardPieceAnnotation.annotate(&[captured]), None);
    for row in [
        feed(0, None, None),
        feed(1, Some(""), None),
        feed(1, Some(" working "), None),
        feed(1, None, Some("bad stored time")),
    ] {
        let transaction = ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(902),
            rows: vec![row],
        };
        let captured = CapturedTransaction {
            position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(902)),
            transaction: &transaction,
        };
        assert!(CapturedProjectChanges::decode(&captured).is_err());
    }
    Ok(())
}
