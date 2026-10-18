#![allow(clippy::expect_used)]

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::*;

pub(super) fn project_id() -> ProjectId {
    ProjectId::new("019f0000-0000-7000-8000-000000000001").expect("valid project")
}
pub(super) fn board_id() -> BoardId {
    BoardId::new("019f0000-0000-7000-8000-000000000004").expect("valid board")
}
pub(super) fn installation_id() -> InstallationId {
    InstallationId::new("019f0000-0000-7000-8000-000000000002").expect("valid installation")
}
pub(super) fn session_id() -> SessionId {
    SessionId::new("019f0000-0000-7000-8000-000000000003").expect("valid session")
}
pub(super) fn frame_id() -> FrameId {
    FrameId::new("019f0000-0000-7000-8000-000000000005").expect("valid frame")
}
pub(super) fn transfer_id() -> TransferId {
    TransferId::new("019f0000-0000-7000-8000-000000000006").expect("valid transfer")
}
pub(super) fn request_id() -> RequestId {
    RequestId::new("019f0000-0000-7000-8000-000000000007").expect("valid request")
}
pub(super) fn epoch(value: u64) -> Epoch {
    Epoch::new(value).expect("valid epoch")
}
pub(super) fn position(epoch_value: u64, commit_id: u64) -> ReplicationPosition {
    ReplicationPosition::new(epoch(epoch_value), CommitId::new(commit_id))
}
pub(super) fn timestamp() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-11T12:34:56Z")
        .expect("valid RFC3339")
        .with_timezone(&Utc)
}
pub(super) fn sample_piece() -> Piece {
    Piece::new(PieceContents {
        project: project_id(),
        epoch: epoch(1),
        continues_from: position(1, 1),
        through: position(1, 2),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: CommitId::new(2),
            rows: vec![
                RowChange::upsert(
                    "records",
                    vec![SqlValue::Text("key".into())],
                    vec![SqlValue::Blob(vec![0, 1, 2, 255])],
                )
                .expect("valid row"),
            ],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "open".into(),
            latest_record_at: timestamp(),
        }),
    })
    .expect("valid piece")
}
pub(super) fn lease_status() -> LeaseStatus {
    LeaseStatus {
        project: project_id(),
        epoch: epoch(1),
        holder: installation_id(),
        state: LeaseState::Held,
        expires_at_registry: timestamp(),
        remaining_ms: 60_000,
    }
}
pub(super) fn snapshot_cursor() -> SnapshotCursor {
    SnapshotCursor {
        position: position(1, 2),
        table: "records".into(),
        after_key: vec![SqlValue::Text("key".into())],
    }
}
pub(super) fn snapshot_page() -> SnapshotPage {
    SnapshotPage {
        project: project_id(),
        position: position(1, 2),
        schema: SchemaIdentity::new(1, "schema-fingerprint".into()).expect("valid schema"),
        schema_steps: vec!["CREATE TABLE records(id TEXT PRIMARY KEY)".into()],
        table: "records".into(),
        rows: vec![vec![SqlValue::Text("key".into())]],
        next: Some(snapshot_cursor()),
        as_of: timestamp(),
    }
}
pub(super) fn assert_variant<TValue: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: TValue,
    tag: &str,
) {
    let encoded = serde_json::to_value(&value).expect("serializable fixture");
    assert_eq!(encoded.get("kind"), Some(&json!(tag)));
    assert_eq!(
        serde_json::from_value::<TValue>(encoded).expect("decodable fixture"),
        value
    );
}
pub(super) fn request_samples() -> Vec<(SessionRequest, &'static str)> {
    vec![
        (SessionRequest::Piece(sample_piece()), "piece"),
        (
            SessionRequest::Renew {
                attempt: request_id(),
                project: project_id(),
                epoch: epoch(1),
            },
            "renew",
        ),
        (
            SessionRequest::Release {
                project: project_id(),
                epoch: epoch(1),
                transfer: transfer_id(),
                outcome: crate::ReleaseOutcome::Final(position(1, 2)),
            },
            "release",
        ),
        (
            SessionRequest::ReserveFirstEpoch {
                project: project_id(),
            },
            "reserveFirstEpoch",
        ),
        (
            SessionRequest::TailReport {
                project: project_id(),
                epoch: epoch(1),
                request_ids: vec![request_id()],
                digest: TailDigest::new("ab".repeat(32)).expect("tail digest"),
            },
            "tailReport",
        ),
        (
            SessionRequest::Fleet(FleetRequestEnvelope {
                request_id: request_id(),
                operation: FleetPayload::new(json!({"kind": "createProject"})),
            }),
            "fleet",
        ),
        (
            SessionRequest::Read {
                scope: ReadScope::FleetRecords,
                after: Some(ReadCursor::Fleet(FleetPosition::new(2))),
            },
            "read",
        ),
        (
            SessionRequest::Read {
                scope: ReadScope::Directory {
                    query: FleetPayload::new(json!({"query": "board"})),
                },
                after: None,
            },
            "read",
        ),
        (
            SessionRequest::Read {
                scope: ReadScope::ProjectCopy {
                    project: project_id(),
                },
                after: Some(ReadCursor::Snapshot(snapshot_cursor())),
            },
            "read",
        ),
        (
            SessionRequest::Read {
                scope: ReadScope::Notices,
                after: Some(ReadCursor::Notices(
                    NoticePosition::new(2).expect("notice position"),
                )),
            },
            "read",
        ),
        (
            SessionRequest::Read {
                scope: ReadScope::TailReports {
                    project: project_id(),
                },
                after: None,
            },
            "read",
        ),
        (
            SessionRequest::ArchiveSeen {
                board: board_id(),
                project: project_id(),
                position: position(1, 2),
            },
            "archiveSeen",
        ),
    ]
}
pub(super) fn refusal_samples() -> Vec<(SessionRefusal, &'static str)> {
    vec![
        (
            SessionRefusal::StaleEpoch {
                current_epoch: epoch(2),
                epoch_ended_at: position(1, 2),
            },
            "staleEpoch",
        ),
        (
            SessionRefusal::NotWriter {
                current: Some(epoch(1)),
            },
            "notWriter",
        ),
        (
            SessionRefusal::PositionGap {
                acked: position(1, 2),
            },
            "positionGap",
        ),
        (SessionRefusal::PieceConflict, "pieceConflict"),
        (SessionRefusal::NotInitialized, "notInitialized"),
        (SessionRefusal::CentralCopyFull, "centralCopyFull"),
        (
            SessionRefusal::ProjectRejected {
                reason: "invalid project".into(),
            },
            "projectRejected",
        ),
        (SessionRefusal::CursorExpired, "cursorExpired"),
        (
            SessionRefusal::TailConflict {
                existing: TailDigest::new("ab".repeat(32)).expect("tail digest"),
            },
            "tailConflict",
        ),
    ]
}
pub(super) fn notice_samples() -> Vec<(NoticeKind, &'static str)> {
    vec![
        (
            NoticeKind::FleetFeedAdvanced {
                through: FleetPosition::new(42),
            },
            "fleetFeedAdvanced",
        ),
        (
            NoticeKind::LeaseSuperseded {
                project: project_id(),
                epoch: epoch(1),
            },
            "leaseSuperseded",
        ),
        (
            NoticeKind::LeaseGranted {
                project: project_id(),
                epoch: epoch(1),
            },
            "leaseGranted",
        ),
        (
            NoticeKind::ReleaseRequested {
                project: project_id(),
                transfer: transfer_id(),
            },
            "releaseRequested",
        ),
        (
            NoticeKind::BoardArchiving { board: board_id() },
            "boardArchiving",
        ),
        (
            NoticeKind::RejectedTailReported {
                project: project_id(),
                epoch: epoch(1),
            },
            "rejectedTailReported",
        ),
    ]
}
pub(super) fn read_page_samples() -> Vec<(ReadPage, &'static str)> {
    vec![
        (
            ReadPage::TailReports {
                reports: vec![TailReportRow {
                    project: project_id(),
                    epoch: epoch(1),
                    request_ids: vec![request_id()],
                    digest: TailDigest::new("ab".repeat(32)).expect("tail digest"),
                }],
                as_of: timestamp(),
            },
            "tailReports",
        ),
        (
            ReadPage::FleetRecords {
                records: vec![FleetPayload::new(json!({"id": "record-1"}))],
                position: FleetPosition::new(4),
                as_of: timestamp(),
            },
            "fleetRecords",
        ),
        (
            ReadPage::Directory {
                entries: vec![FleetPayload::new(json!({"project": "project-1"}))],
                as_of: timestamp(),
            },
            "directory",
        ),
        (
            ReadPage::ProjectCopy(ProjectCopyRead::Page(Box::new(snapshot_page()))),
            "projectCopy",
        ),
        (
            ReadPage::ProjectCopy(ProjectCopyRead::Moved {
                position: position(2, 3),
            }),
            "projectCopy",
        ),
        (
            ReadPage::Notices {
                notices: vec![Notice {
                    position: NoticePosition::new(5).expect("notice position"),
                    kind: NoticeKind::LeaseGranted {
                        project: project_id(),
                        epoch: epoch(1),
                    },
                }],
                as_of: timestamp(),
            },
            "notices",
        ),
    ]
}
