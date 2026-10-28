//! Archive admission reads canonical children, leases and reservations together.
use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequestResult};
use message_board::{BoardState, Record, RequestDecision, WorkspaceState};
use serde_json::json;
use sqlx_core::{query::query, row::Row};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn workspace_archive_keeps_unknown_lease_blockers_and_writes_more_than_five_row_ids()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let mut offered = bundle()?;
        for ordinal in 0..4 {
            offered.records.push(serde_json::from_value(json!({
                "kind":"board",
                "id":format!("019f0000-0000-7000-8000-{:012x}",0xc10+ordinal),
                "workspace":"019f0000-0000-7000-8000-000000000a81",
                "name":format!("Empty board {ordinal}"),
                "description":"archive input fixture","state":"active",
                "time":{"kind":"legacyUnknown"}
            }))?);
        }
        fixture.import(&offered).await?;
        let archive = request(FleetOperation::WorkspaceArchive {})?;
        let answer = fixture.fleet_request(&archive).await?;
        assert!(matches!(&answer, FleetRequestResult::Decided(RequestDecision::Accepted { .. })));
        assert_eq!(fixture.fleet_request(&archive).await?, answer);
        let (workspace, boards, ledgers, receipts) = fixture.object.transaction(|scope|Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT payload,(SELECT count(*) FROM fleet_request_decisions) AS ledgers,(SELECT count(*) FROM fleet_decision_receipts) AS receipts FROM workspaces")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            let workspace = row.try_get::<String,_>("payload").map_err(StoreError::backend)?;
            let ledgers = row.try_get::<i64,_>("ledgers").map_err(StoreError::backend)?;
            let receipts = row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?;
            let boards = query::<sqlx_turso::Turso>("SELECT payload FROM boards ORDER BY board_id").fetch_all(scope.executor()).await.map_err(StoreError::backend)?
                .into_iter().map(|row|row.try_get::<String,_>("payload").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
            Ok((workspace,boards,ledgers,receipts))
        })).await?;
        let Record::Workspace(workspace) = serde_json::from_str(&workspace)? else { return Err("workspace record absent".into()); };
        assert_eq!(workspace.state, WorkspaceState::Archiving);
        assert_eq!(boards.len(),5);
        let boards = boards.into_iter().map(|payload|serde_json::from_str::<Record>(&payload)).collect::<Result<Vec<_>,_>>()?;
        for (ordinal, record) in boards.into_iter().enumerate() {
            let Record::Board(board) = record else { return Err("board record absent".into()); };
            assert_eq!(board.state,if ordinal==0 { BoardState::Archiving } else { BoardState::Archived });
        }
        assert_eq!((ledgers,receipts),(1,6));
        assert_eq!(fixture.counts().await?,vec![1,5,1,1,14]);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
