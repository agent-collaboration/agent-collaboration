//! Feed reads expose the canonical committed prefix through a real admitted session.
use super::*;
use board_sync_protocol::{BoardReadCursor, BoardReadScope};
use message_board::{FleetPosition, RequestId, WorkspaceId};
use serde_json::{Value, json};

pub(super) fn offered(operation: FleetOperation) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::try_from(Uuid::now_v7().to_string())?,
        workspace: WorkspaceId::try_from(WORKSPACE.to_owned())?,
        actor: message_board::RecordedActor::Human {
            human_id: message_board::HumanId::try_from("feed fixture owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation,
    })
}
pub(super) fn create() -> ConformanceResult<FleetRequest> {
    offered(FleetOperation::WorkspaceCreate {
        name: message_board::ResourceName::try_from("Read the committed fleet".to_owned())?,
        description: message_board::Description::try_from("Real read fixture".to_owned())?,
    })
}
async fn feed(socket: &mut WebSocket, after: u64) -> ConformanceResult<Value> {
    let answer = operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(BoardReadCursor::Fleet(FleetPosition::try_from(after)?)),
        }),
    )
    .await?;
    let answer = serde_json::to_value(answer)?;
    answer
        .get("value")
        .and_then(|answer| answer.get("value"))
        .filter(|answer| answer.get("kind") == Some(&json!("fleetFeed")))
        .and_then(|answer| answer.get("value"))
        .cloned()
        .ok_or_else(|| format!("Fleet read returned a different typed operation: {answer}").into())
}
fn page(answer: &Value) -> ConformanceResult<&Value> {
    if answer.get("kind") != Some(&json!("page")) {
        return Err(format!("expected a feed page, got {answer}").into());
    }
    answer
        .get("value")
        .ok_or_else(|| "missing feed page body".into())
}
fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("Fleet read returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) async fn accepted(
    socket: &mut WebSocket,
    offered: &FleetRequest,
) -> ConformanceResult<()> {
    let result = decided(
        request(socket, offered, offered.request_id.clone()).await?,
        offered,
    )?;
    if !matches!(
        result,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ) {
        return Err(format!("Fleet read fixture metadata was not committed: {result:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn normal_feed_starts_after_the_committed_cursor_and_refuses_a_cursor_beyond_head()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof=async {
        let client=ConformanceClient::new(server.base_url())?;
        let mut peer=enroll_admitted_peer(&client,SERVICE,"019f0000-0000-7000-8000-00000000100e","feed-origin","019f0000-0000-7000-8000-000000000f44").await?;
        let proof=async {
            let empty=feed(&mut peer.socket,0).await?;
            let empty=page(&empty)?;
            require(empty.get("records").cloned().ok_or("missing records")?,json!([]))?;
            require(empty.get("through").cloned().ok_or("missing through")?,json!(0))?;
            require(empty.get("head").cloned().ok_or("missing head")?,json!(0))?;
            let offered=create()?;
            accepted(&mut peer.socket,&offered).await?;
            let all=feed(&mut peer.socket,0).await?;
            let all=page(&all)?;
            require(all.get("through").cloned().ok_or("missing through")?,json!(2))?;
            require(all.get("head").cloned().ok_or("missing head")?,json!(2))?;
            let records:Vec<Record>=serde_json::from_value(all.get("records").cloned().ok_or("missing records")?)?;
            let positions=records.iter().map(|record|record.envelope().position.fleet_position().map(|position|position.get())).collect::<Vec<_>>();
            if positions!=vec![Some(1),Some(2)] || !matches!(records.first(),Some(Record::Workspace(workspace)) if workspace.workspace_id.as_str()==WORKSPACE) {
                return Err("feed lost its literal committed order or canonical workspace".into());
            }
            if !matches!(records.last(),Some(Record::Request(decision)) if decision.request_id==offered.request_id) {return Err("feed omitted its committed ledger record".into());}
            let suffix=feed(&mut peer.socket,1).await?;
            let suffix=page(&suffix)?;
            let records:Vec<Record>=serde_json::from_value(suffix.get("records").cloned().ok_or("missing suffix")?)?;
            if records.len()!=1 || !matches!(records.first(),Some(Record::Request(decision)) if decision.request_id==offered.request_id) {return Err("feed repeated the record before its cursor".into());}
            let caught_up=feed(&mut peer.socket,2).await?;
            require(page(&caught_up)?.get("records").cloned().ok_or("missing caught-up records")?,json!([]))?;
            require(feed(&mut peer.socket,3).await?,json!({"kind":"cursorInvalid"}))?;
            let live=feed(&mut peer.socket,2).await?;
            require(page(&live)?.get("through").cloned().ok_or("missing live cursor")?,json!(2))?;
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _=peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn normal_expired_feed_cursor_returns_its_floor_after_real_retention_then_keeps_the_session_live()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let first = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let client = ConformanceClient::new(first.base_url())?;
        let mut peer = enroll_admitted_peer(
            &client,
            SERVICE,
            "019f0000-0000-7000-8000-00000000100e",
            "expired-feed-origin",
            "019f0000-0000-7000-8000-000000000f45",
        )
        .await?;
        let proof = async {
            let offered = create()?;
            accepted(&mut peer.socket, &offered).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let _ = peer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof
    }
    .await;
    first.shutdown().await?;
    proof?;
    // The owning server is stopped. Only feed-retention bookkeeping is aged;
    // the real next metadata transaction runs the production retention rule.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join("fleet.db"))
        .connect()
        .await?;
    query::<sqlx_turso::Turso>("UPDATE fleet_feed SET as_of='2020-01-01T00:00:00.000000000Z'")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let second = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof=async {
        let client=ConformanceClient::new(second.base_url())?;
        let mut peer=enroll_admitted_peer(&client,SERVICE,"019f0000-0000-7000-8000-00000000100e","expired-feed-origin","019f0000-0000-7000-8000-000000000f46").await?;
        let proof=async {
            // No new mutation may be needed to apply the thirty-day retention rule.
            require(feed(&mut peer.socket,0).await?,json!({"kind":"cursorExpired","value":{"floor":2}}))?;
            let quiet=feed(&mut peer.socket,2).await?;
            let quiet=page(&quiet)?;
            require(quiet.get("records").cloned().ok_or("missing idle records")?,json!([]))?;
            require(quiet.get("head").cloned().ok_or("missing idle head")?,json!(2))?;
            let update=offered(FleetOperation::WorkspaceUpdate {name:message_board::ResourceName::try_from("Updated after retention".to_owned())?,description:message_board::Description::try_from("Current state survives feed pruning".to_owned())?})?;
            accepted(&mut peer.socket,&update).await?;
            require(feed(&mut peer.socket,0).await?,json!({"kind":"cursorExpired","value":{"floor":2}}))?;
            let current=feed(&mut peer.socket,2).await?;
            let current=page(&current)?;
            require(current.get("through").cloned().ok_or("missing current through")?,json!(4))?;
            require(current.get("head").cloned().ok_or("missing current head")?,json!(4))?;
            let records:Vec<Record>=serde_json::from_value(current.get("records").cloned().ok_or("missing current records")?)?;
            if !matches!(records.first(),Some(Record::Workspace(workspace)) if workspace.name.as_str()=="Updated after retention") {return Err("retention lost the current canonical workspace".into());}
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _=peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await;
    second.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
