//! Both ordered sides of Fence retain complete literal multi-row prefixes.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "transfer_boundaries/fixture.rs"]
mod fixture;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, RuntimeControl, enroll_admitted_peer,
    fixture_policy, machine_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

const LEFT_KEY: &str = "019f0000-0000-7000-8000-000000000797";
const RIGHT_KEY: &str = "019f0000-0000-7000-8000-000000000798";
fn values(left: &str, right: &str) -> Vec<Vec<SqlValue>> {
    vec![
        vec![
            SqlValue::Text(LEFT_KEY.into()),
            SqlValue::Integer(1),
            SqlValue::Text(left.into()),
        ],
        vec![
            SqlValue::Text(RIGHT_KEY.into()),
            SqlValue::Integer(2),
            SqlValue::Text(right.into()),
        ],
    ]
}
fn piece(
    database: DatabaseId,
    epoch: u64,
    left: &str,
    right: &str,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    let through = if epoch == 1 {
        fixture::position(1, 2)?
    } else {
        fixture::position(2, 1)?
    };
    let rows = values(left, right)
        .into_iter()
        .zip([LEFT_KEY, RIGHT_KEY])
        .map(|(row, key)| RowChange::upsert("transfer_rows", vec![SqlValue::Text(key.into())], row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Piece::new(PieceContents {
        database,
        epoch: Epoch::new(epoch)?,
        continues_from: fixture::position(1, 1)?,
        through,
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: through.sequence,
            rows,
        }],
        part: None,
        annotation: None,
    })?)
}
#[tokio::test]
async fn complete_two_row_piece_is_read_before_the_held_fence_is_refused_preview_stale()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof=async {
        let request=fixture.takeover(fixture.preview().await?,fixture::TARGET_SERVICE);
        fixture.control(RuntimeControl::HoldNextDispatch {destination:ObjectAddress::Copy(fixture.database)}).await?;
        let http=fixture.http.clone();let route=fixture.route("takeover");let sending=request.clone();
        callers.spawn(async move {owner_registry_scenarios::post(&http,&route,&sending).await});
        let held=fixture.held_copy().await?;
        let fence:FenceRequest=serde_json::from_value(held.get("request").cloned().ok_or("ordered held Fence omitted request")?)?;
        if held.get("kind")!=Some(&json!("fleet-sync.copy-fence")) || fence.database!=fixture.database || request.get("transferId")!=Some(&json!(fence.transfer_id))
            || fence.new_epoch!=Epoch::new(2)? || fence.expect!=(FenceExpectation {epoch:Epoch::new(1)?,acked:fixture::position(1,1)?}) || fence.holder!=PeerId::try_from(fixture::TARGET_PEER.to_owned())? {return Err("ordered-before hold did not select the exact original Fence2/preview1:1".into());}
        let rows=piece(fixture.database,1,"before-left","before-right")?;
        if !matches!(creation::operation_answer(&mut fixture.old,OperationRequest::Replication(ReplicationRequest::Piece(rows))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==fixture::position(1,2)?) {return Err("ordered-before Piece did not apply the complete two-row prefix1:2".into());}
        fixture.copy(1,fixture::position(1,2)?,values("before-left","before-right")).await?;
        println!("TRANSFER_ORDERED_STAGE complete_before_left_right_read_while_exact_Fence_held passed");
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("ordered-before Takeover caller vanished")???;
        let refused=fixture.complete("takeover",&request,initial).await?;
        if refused!=json!({"kind":"refused","value":{"kind":"previewStale"}}) {return Err(format!("ordered-before Fence did not refuse its now-stale preview: {refused}").into());}
        fixture.copy(1,fixture::position(1,2)?,values("before-left","before-right")).await?;
        println!("TRANSFER_ORDERED_CASE whole_Piece_before_Fence_preserves_rows_and_refuses_stale_preview passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = fixture.close().await;
    proof?;
    closed?;
    Ok(())
}
#[tokio::test]
async fn granted_fence_rejects_whole_late_rows_then_the_new_holder_commits_both_rows()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof=async {
        let request=fixture.takeover(fixture.preview().await?,fixture::TARGET_SERVICE);
        let initial=fixture.post("takeover",&request).await?;
        let granted=fixture.complete("takeover",&request,initial).await?;
        if granted!=json!({"kind":"granted","value":{"newEpoch":2,"start":{"epoch":1,"sequence":1}}}) {return Err(format!("ordered-after Takeover did not grant exact2/start1:1: {granted}").into());}
        let late=piece(fixture.database,1,"late-left","late-right")?;
        let refused=creation::operation_answer(&mut fixture.old,OperationRequest::Replication(ReplicationRequest::Piece(late))).await?;
        if !matches!(refused,OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer,ended_at})) if newer==Epoch::new(2)? && ended_at==fixture::position(1,1)?) {return Err("ordered-after late two-row Piece lost whole StaleEpoch2/cutoff1:1".into());}
        fixture.copy(2,fixture::position(1,1)?,vec![]).await?;
        println!("TRANSFER_ORDERED_STAGE whole_late_left_right_refused_and_empty_original_prefix_read passed");
        let fresh=piece(fixture.database,2,"new-left","new-right")?;
        if !matches!(creation::operation_answer(&mut fixture.target,OperationRequest::Replication(ReplicationRequest::Piece(fresh))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==fixture::position(2,1)?) {return Err("ordered-after new holder did not commit both rows at2:1".into());}
        let page=creation::operation_answer(&mut fixture.target,OperationRequest::Replication(ReplicationRequest::SnapshotPage {database:fixture.database,cursor:None})).await?;
        if !matches!(page,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==fixture.database && page.position==fixture::position(2,1)? && page.schema.version==1 && page.table=="transfer_rows" && page.rows==values("new-left","new-right") && page.next.is_none()) {return Err("ordered-after target snapshot lost exact new-left/right or exposed any late row".into());}
        fixture.copy(2,fixture::position(2,1)?,values("new-left","new-right")).await?;
        println!("TRANSFER_ORDERED_CASE Fence_before_late_Piece_preserves_empty_prefix_and_new_holder_rows passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed?;
    Ok(())
}
