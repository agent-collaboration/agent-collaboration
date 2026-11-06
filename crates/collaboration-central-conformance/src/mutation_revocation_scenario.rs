//! Shared wire proof: committed revocation refuses new work while the old session is attached.
use crate::revocation_race_scenarios::{control, live, observed, receive};
use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion};
use collaboration_central_core::RevocationControl;
use futures_util::SinkExt;
use reqwest_websocket::{CloseCode, Message, WebSocket};
use serde_json::{Value, json};
use tokio::task::JoinSet;

const SENDER: &str = "550e8400-e29b-41d4-a716-4466554400e9";
const SESSION: &str = "019f0000-0000-7000-8000-000000000983";

pub async fn run_mutation_revocation_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut sender = enroll_admitted_peer(
        client,
        SENDER,
        "019f0000-0000-7000-8000-000000001008",
        "revoked-mail-sender",
        SESSION,
    )
    .await?;
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut recipient = enroll_admitted_peer(
        client,
        &service,
        &durable_steps::Uuid::now_v7().to_string(),
        "revoked-mail-recipient",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let mut requests = JoinSet::new();
    let proof = async {
        control(client, RevocationControl::ArmDelivery).await?;
        let caller = client.clone();
        requests.spawn(async move {
            Ok::<Value, Box<dyn std::error::Error + Send + Sync>>(caller.client
                .post(format!("{}/admin/v1/machines/revoke", caller.base_url))
                .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                .json(&json!({"kind":"revokeMachine","value":{"installation":SENDER}}))
                .send().await?.error_for_status()?.json().await?)
        });
        let held = observed(client, |value| value.gate_armed && value.gate_reached
            && value.enrollment.as_ref().is_some_and(|row| row.revoked)
            && matches!(value.operation.as_ref().map(|operation| &operation.state),
                Some(durable_steps::OperationState::Running { .. }))).await?;
        // A normal Hello proves that this is the still-attached old session.
        live(&mut sender.socket, SESSION).await?;
        revoked_fleet_request_selects_only_its_admission_refusal(&mut sender.socket).await?;
        revoked_archive_seen_selects_only_its_admission_refusal(&mut sender.socket).await?;
        revoked_forward_selects_no_pending_row_or_route(&mut sender.socket).await?;
        revoked_project_settle_selects_no_negative_custody(&mut sender.socket).await?;
        revoked_register_selects_no_registration(&mut sender.socket).await?;
        revoked_renew_selects_no_renewal(&mut sender.socket).await?;
        revoked_claim_selects_no_transfer(&mut sender.socket).await?;
        revoked_seed_begin_selects_no_copy_write(&mut sender.socket).await?;
        revoked_piece_selects_no_copy_write(&mut sender.socket).await?;
        revoked_seed_page_selects_no_copy_write(&mut sender.socket).await?;
        revoked_seed_complete_selects_no_copy_write(&mut sender.socket).await?;
        if control(client, RevocationControl::Observe).await?.outbox_items != held.outbox_items { return Err("mutation refusal changed outbox".into()); }
        // The correlated recipient barrier rejects any Push instead of silently
        // skipping it, and requires no timeout to claim absence.
        let barrier = operation(&mut recipient.socket, json!({"kind":"delivery","value":{
            "kind":"statusFetch","value":{"call":{"operation":durable_steps::Uuid::now_v7(),
            "step":"revoked_sender_barrier"},"after":0,"limit":10}}})).await?;
        if barrier != json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[],"through":0}}}) {
            return Err(format!("refused sender produced recipient traffic: {barrier}").into());
        }
        println!("MUTATION_ENROLLMENT_CASE eleven_new_shared_mutations_refused_in_live_revoked_session passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let released = control(client, RevocationControl::ReleaseDelivery).await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    let _ = sender.socket.close(CloseCode::Normal, None).await;
    let _ = recipient.socket.close(CloseCode::Normal, None).await;
    proof?;
    released?;
    Ok(())
}

async fn operation(socket: &mut WebSocket, body: Value) -> ConformanceResult<Value> {
    let frame = durable_steps::Uuid::now_v7();
    socket
        .send(Message::Text(
            json!({"kind":"request","value":{"frame":frame,"body":body}}).to_string(),
        ))
        .await?;
    let answer = receive(socket).await?;
    if answer.get("kind") != Some(&json!("answer"))
        || answer.pointer("/value/frame") != Some(&json!(frame))
    {
        return Err(format!("expected exact correlated answer, got {answer}").into());
    }
    Ok(answer
        .pointer("/value/body")
        .cloned()
        .ok_or("missing answer body")?)
}

const PROJECT: &str = "019f0000-0000-7000-8000-00000000a103";
const PEER: &str = "019f0000-0000-7000-8000-00000000a101";
async fn expect_refusal(
    socket: &mut WebSocket,
    body: Value,
    expected: Value,
) -> ConformanceResult<()> {
    let actual = operation(socket, body).await?;
    if actual != expected {
        return Err(format!("wrong mutation refusal: {actual}; expected {expected}").into());
    }
    Ok(())
}
async fn revoked_fleet_request_selects_only_its_admission_refusal(
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"consumer","value":{"kind":"fleet","value":{"requestId":PROJECT,"operation":fleet_request()?}}}),json!({"kind":"consumer","value":{"kind":"fleet","value":{"requestId":PROJECT,"decision":{"kind":"callerNotEnrolled"}}}})).await
}
async fn revoked_archive_seen_selects_only_its_admission_refusal(
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"consumer","value":{"kind":"archiveSeen","value":{"board":PROJECT,"project":PROJECT,"position":{"epoch":1,"sequence":0}}}}),json!({"kind":"consumer","value":{"kind":"archiveSeen","value":{"kind":"refused","value":{"kind":"callerNotEnrolled"}}}})).await
}
async fn revoked_forward_selects_no_pending_row_or_route(
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"consumer","value":{"kind":"forwardedPost","value":{"project":PROJECT,"post":{"kind":"threadPost","body":{}}}}}),json!({"kind":"consumer","value":{"kind":"forwardedPost","value":{"kind":"callerNotEnrolled"}}})).await
}
async fn revoked_project_settle_selects_no_negative_custody(
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"delivery","value":{"kind":"settle","value":{"call":{"operation":PROJECT,"step":"settle"},"submitter":{"kind":"project","value":PROJECT},"mailbox":PEER,"item":PROJECT,"digest":"ab".repeat(32)}}}),json!({"kind":"delivery","value":{"kind":"deliveryUnavailable","value":{"reason":{"kind":"senderNotEnrolled"}}}})).await
}

fn gate_reason() -> Value {
    json!({"code":"callerNotEnrolled","detail":"Machine is not enrolled or its enrollment has expired."})
}
fn replication_refusal() -> Value {
    json!({"kind":"replication","value":{"kind":"refused","value":{"kind":"hook","value":gate_reason()}}})
}
async fn revoked_register_selects_no_registration(socket: &mut WebSocket) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"lease","value":{"kind":"register","value":{"attempt":PROJECT,"database":PROJECT,"writer":PEER,"origin":{"kind":"empty"}}}}),json!({"kind":"lease","value":{"kind":"register","value":{"kind":"refused","value":{"kind":"databaseRejected","value":{"reason":gate_reason()}}}}})).await
}
async fn revoked_renew_selects_no_renewal(socket: &mut WebSocket) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"lease","value":{"kind":"renew","value":{"attempt":PROJECT,"database":PROJECT,"epoch":1}}}),json!({"kind":"lease","value":{"kind":"renew","value":{"kind":"refused","value":{"kind":"holderNotEnrolled"}}}})).await
}
async fn revoked_claim_selects_no_transfer(socket: &mut WebSocket) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"lease","value":{"kind":"claim","value":{"attempt":PROJECT,"database":PROJECT}}}),json!({"kind":"lease","value":{"kind":"claim","value":{"kind":"refused","value":{"kind":"gateRefused","value":{"reason":gate_reason()}}}}})).await
}
async fn revoked_seed_begin_selects_no_copy_write(socket: &mut WebSocket) -> ConformanceResult<()> {
    expect_refusal(socket, json!({"kind":"replication","value":{"kind":"seedBegin","value":{"database":PROJECT,"attempt":PROJECT}}}),replication_refusal()).await
}
fn piece() -> ConformanceResult<Value> {
    use fleet_sync_protocol::{
        CommitSequence, Epoch, Piece, PieceContents, ReplicatedTransaction, ReplicationPosition,
    };
    let epoch = Epoch::new(1)?;
    let contents = PieceContents::<board_sync_protocol::BoardAnnotation> {
        database: PROJECT.to_owned().try_into()?,
        epoch,
        continues_from: ReplicationPosition::new(epoch, CommitSequence::new(0)),
        through: ReplicationPosition::new(epoch, CommitSequence::new(1)),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(1),
            rows: Vec::new(),
        }],
        part: None,
        annotation: None,
    };
    Ok(serde_json::to_value(Piece::new(contents)?)?)
}
async fn revoked_piece_selects_no_copy_write(socket: &mut WebSocket) -> ConformanceResult<()> {
    expect_refusal(
        socket,
        json!({"kind":"replication","value":{"kind":"piece","value":piece()?}}),
        replication_refusal(),
    )
    .await
}
fn page() -> ConformanceResult<fleet_sync_protocol::SnapshotPage> {
    Ok(fleet_sync_protocol::SnapshotPage {
        database: PROJECT.to_owned().try_into()?,
        position: fleet_sync_protocol::ReplicationPosition::new(
            fleet_sync_protocol::Epoch::new(1)?,
            fleet_sync_protocol::CommitSequence::new(0),
        ),
        schema: fleet_sync_protocol::SchemaIdentity::new(1, "gate-fixture".into())?,
        schema_steps: Vec::new(),
        table: "".into(),
        rows: Vec::new(),
        next: None,
    })
}
async fn revoked_seed_page_selects_no_copy_write(socket: &mut WebSocket) -> ConformanceResult<()> {
    let page = page()?;
    let request = fleet_sync_protocol::SeedPage {
        seed: fleet_sync_protocol::SeedId { generation: 1 },
        page: 0,
        digest: fleet_sync_protocol::PageDigest::of_canonical(&page)?,
        contents: page,
    };
    expect_refusal(
        socket,
        json!({"kind":"replication","value":{"kind":"seedPage","value":request}}),
        replication_refusal(),
    )
    .await
}
async fn revoked_seed_complete_selects_no_copy_write(
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    let page = page()?;
    expect_refusal(socket, json!({"kind":"replication","value":{"kind":"seedComplete","value":{"database":PROJECT,"seed":{"generation":1},"position":page.position,"pages":1,"schema":page.schema,"tableDigests":[]}}}),replication_refusal()).await
}

fn fleet_request() -> ConformanceResult<Value> {
    Ok(serde_json::to_value(fleet_admission::FleetRequest {
        request_id: PROJECT.to_owned().try_into()?,
        workspace: PROJECT.to_owned().try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "revocation fixture".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: fleet_admission::FleetOperation::WorkspaceCreate {
            name: "revoked creation".to_owned().try_into()?,
            description: "must select no write".to_owned().try_into()?,
        },
    })?)
}
