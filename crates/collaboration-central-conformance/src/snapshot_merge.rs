//! A normal-session snapshot crosses both record-kind and public page boundaries.
use crate::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardReadCursor, BoardReadPage, BoardReadScope,
};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::{
    BoardId, BoardKind, Description, HumanId, Identity, ProjectId, ProtocolVersion, Record,
    RecordedActor, RequestDecision, RequestId, ResourceName, WorkspaceId,
};

pub async fn run_fleet_snapshot_merge_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut session = NativeBoardSession(
        enroll_admitted_peer(
            client,
            "550e8400-e29b-41d4-a716-446655440af3",
            "019f0000-0000-7000-8000-000000001002",
            "snapshot-merge-proof",
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof = async {
        let early = "00000000-0000-7000-8000-000000000001";
        let late = "ffffffff-ffff-7fff-8fff-fffffffffff1";
        let workspace = WorkspaceId::try_from(early.to_owned())?;
        let board = BoardId::try_from(early.to_owned())?;
        let project = ProjectId::try_from(early.to_owned())?;
        let late_workspace = WorkspaceId::try_from(late.to_owned())?;
        let description = Description::try_from("snapshot merge page boundary".to_owned())?;
        let mut operations = vec![
            (workspace.clone(), FleetOperation::WorkspaceCreate { name: ResourceName::try_from("Early snapshot workspace".to_owned())?, description: description.clone() }),
            (workspace.clone(), FleetOperation::BoardCreate { board: board.clone(), name: ResourceName::try_from("Snapshot board".to_owned())?, description: description.clone(), board_kind: BoardKind::Area {} }),
            (workspace.clone(), FleetOperation::ProjectDirectoryCreate { project, board, title: ResourceName::try_from("Snapshot project".to_owned())? }),
            (late_workspace.clone(), FleetOperation::WorkspaceCreate { name: ResourceName::try_from("Late snapshot workspace".to_owned())?, description: description.clone() }),
        ];
        for ordinal in 1..=253 {
            operations.push((late_workspace.clone(), FleetOperation::WorkspaceUpdate {
                name: ResourceName::try_from(format!("Snapshot revision {ordinal}"))?, description: description.clone(),
            }));
        }
        let mut expected = vec![(early.to_owned(), "board".to_owned()), (early.to_owned(), "project".to_owned()), (early.to_owned(), "workspace".to_owned())];
        for (ordinal, (workspace, operation)) in operations.into_iter().enumerate() {
            let id = format!("80000000-0000-7000-8000-{ordinal:012x}");
            let request = FleetRequest {
                request_id: RequestId::try_from(id.clone())?, workspace,
                actor: RecordedActor::declared(Identity::Human { human_id: HumanId::try_from("snapshot requester".to_owned())? }),
                acting_for: None, protocol_version: ProtocolVersion::CURRENT, operation,
            };
            if !matches!(session.write(&request).await?, RequestDecision::Accepted { .. }) {
                return Err("snapshot fixture metadata was not accepted".into());
            }
            expected.push((id, "request".to_owned()));
        }
        expected.push((late.to_owned(), "workspace".to_owned()));
        let mut cursor = None;
        let mut cut = None;
        let mut observed = Vec::new();
        let mut page_sizes = Vec::new();
        loop {
            let BoardConsumerAnswer::Read(BoardReadPage::FleetSnapshot { records, next, snapshot_at, .. }) = session.request(BoardConsumerRequest::Read {
                scope: BoardReadScope::FleetSnapshot, cursor: cursor.clone(),
            }).await? else { return Err("snapshot merge did not answer its page family".into()); };
            if cut.is_some_and(|cut| cut != snapshot_at) { return Err("snapshot continuation changed its cut".into()); }
            cut = Some(snapshot_at);
            page_sizes.push(records.len());
            for payload in records {
                let record: Record = payload.decode()?;
                let reference = record.reference();
                let kind = serde_json::to_value(reference.kind)?;
                observed.push((reference.id.as_str().to_owned(), kind.as_str().ok_or("record kind is not text")?.to_owned()));
            }
            if let Some(next) = next {
                let last_kind = serde_json::to_value(next.after.kind)?;
                let last = (next.after.id.as_str().to_owned(), last_kind.as_str().ok_or("cursor kind is not text")?.to_owned());
                if observed.last() != Some(&last) || Some(BoardReadCursor::FleetSnapshot(next.clone())) == cursor {
                    return Err("snapshot cursor did not name and advance from its last emitted key".into());
                }
                cursor = Some(BoardReadCursor::FleetSnapshot(next));
            } else { break; }
        }
        if observed != expected || page_sizes != [256, 5] {
            return Err(format!("snapshot merge lost global identity/kind order or page bound: {} records, pages {page_sizes:?}", observed.len()).into());
        }
        println!("SNAPSHOT merge: 261 exact keys, equal-ID board/project/workspace order, 256/5 page split and stable cut: passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = session.close().await;
    proof?;
    closed
}
