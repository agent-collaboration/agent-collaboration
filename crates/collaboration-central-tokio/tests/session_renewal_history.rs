//! Recorded renewal success stays distinct from current authority and lease expiry.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    AnswerFaultObservation, ConformanceClient, ConformanceResult, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation, enroll_admitted_peer,
    fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, ServiceId, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::json;
use std::time::Duration;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440747";
const PEER: &str = "019f0000-0000-7000-8000-000000000747";
const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440748";
const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000000748";
const CREDENTIAL: &str = "recorded renewal history writer";
const START: &str = "2030-03-17T17:46:40Z";

#[derive(Clone, Copy)]
enum RenewalCase {
    Supersession,
    Expiry,
}

fn time_after_start(elapsed_ms: i64) -> ConformanceResult<SyncTime> {
    let start: chrono::DateTime<chrono::Utc> = START.parse()?;
    Ok(SyncTime::from_unix_millis(
        start.timestamp_millis() + elapsed_ms,
    ))
}
fn original_prefix() -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(1),
    ))
}
fn expected_lease_status(
    database: DatabaseId,
    epoch: u64,
    holder: &str,
    state: LeaseState,
    expiry_ms: i64,
    remaining_ms: u64,
) -> ConformanceResult<LeaseStatus> {
    Ok(LeaseStatus {
        database,
        epoch: Epoch::new(epoch)?,
        holder: PeerId::try_from(holder.to_owned())?,
        state,
        expires_at: time_after_start(expiry_ms)?,
        remaining_ms,
    })
}
async fn runtime_control(
    base: &str,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn pin_registry(base: &str, elapsed_ms: i64) -> ConformanceResult<()> {
    let now = chrono::DateTime::from_timestamp_millis(time_after_start(elapsed_ms)?.unix_millis())
        .ok_or("renewal fixture time is outside range")?;
    runtime_control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::SetTime {
            now: RegistryTime(now),
        },
    )
    .await?;
    Ok(())
}
async fn reconnect_writer(
    base: &str,
    socket: &mut WebSocket,
) -> ConformanceResult<Vec<DatabaseStatus>> {
    // Close the old connection before a fresh normal authenticated Hello.
    SinkExt::close(socket).await?;
    let mut replacement = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(CREDENTIAL)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(CREDENTIAL)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let session = SessionId::new(Uuid::now_v7())?;
    replacement
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id: session,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    let message = tokio::time::timeout(Duration::from_secs(5), replacement.next())
        .await?
        .ok_or("renewal reconnect ended before Welcome")??;
    let welcome: ServerFrame<BoardConsumerProtocol> = match message {
        Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
        Message::Text(text) => serde_json::from_str(&text)?,
        _ => return Err("renewal reconnect did not return its normal Welcome".into()),
    };
    match welcome {
        ServerFrame::Welcome {
            session_id,
            databases,
        } if session_id == session => {
            *socket = replacement;
            Ok(databases)
        }
        _ => Err("renewal reconnect lost its exact Hello session identity".into()),
    }
}
async fn renew_over_session(
    socket: &mut WebSocket,
    request: RenewRequest,
) -> ConformanceResult<RenewAnswer> {
    match creation::operation_answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Renew(request)),
    )
    .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Renew(answer)) => Ok(answer),
        other => Err(format!(
            "normal renewal returned another answer family: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}
fn expected_renewal(
    database: DatabaseId,
    recorded_ms: i64,
    current: LeaseStatus,
    replayed: bool,
) -> ConformanceResult<RenewAnswer> {
    if current.database != database {
        return Err("renewal oracle addresses another database".into());
    }
    Ok(RenewAnswer::Renewed {
        recorded: RenewResult {
            epoch: Epoch::new(1)?,
            expires_at: time_after_start(recorded_ms)?,
        },
        current,
        transfer_in_progress: None,
        replayed,
    })
}
async fn snapshot_unchanged(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    let answer = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?;
    if !matches!(answer,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==database && page.position==original_prefix()? && page.schema.version==1 && page.table=="renewal_records" && page.rows.is_empty() && page.next.is_none())
    {
        return Err(
            "renewal history changed the original schema or empty committed snapshot".into(),
        );
    }
    Ok(())
}
async fn lost_replay_fresh_extension_and_supersession(
    base: &str,
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    // Arrange an actually committed R1 whose outgoing session answer is lost.
    let r1 = RenewRequest {
        attempt: RequestId::new(Uuid::now_v7())?,
        database,
        epoch: Epoch::new(1)?,
    };
    let frame = FrameId::new(Uuid::now_v7())?;
    let session = ObjectAddress::Connection(ServiceId::try_from(SERVICE.to_owned())?);
    runtime_control(
        base,
        session.clone(),
        RuntimeControl::DropNextAnswer { frame },
    )
    .await?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Lease(LeaseRequest::Renew(r1.clone())),
            })?
            .into(),
        ))
        .await?;
    if !matches!(runtime_control(base,session,RuntimeControl::AwaitDroppedAnswer {frame}).await?.answer,AnswerFaultObservation::Dropped {frame:actual} if actual==frame)
    {
        return Err("renewal R1 did not lose its exact committed outgoing answer".into());
    }
    println!("RENEWAL_HISTORY_STAGE original_R1_answer_dropped passed");
    pin_registry(base, 900_000).await?;
    let welcome = reconnect_writer(base, socket).await?;
    let original =
        expected_lease_status(database, 1, PEER, LeaseState::Held, 14_400_000, 13_500_000)?;
    if welcome
        != vec![DatabaseStatus::Ready {
            database,
            acked: original_prefix()?,
            lease: original.clone(),
        }]
    {
        return Err(format!(
            "R1 reconnect Welcome changed original expiry or authority: {welcome:?}"
        )
        .into());
    }
    // Act: replay never extends; a genuinely fresh R2 does extend by fifteen minutes.
    if renew_over_session(socket, r1.clone()).await?
        != expected_renewal(database, 14_400_000, original, true)?
    {
        return Err("replayed R1 changed its recorded expiry or extended the current lease".into());
    }
    let r2 = RenewRequest {
        attempt: RequestId::new(Uuid::now_v7())?,
        database,
        epoch: Epoch::new(1)?,
    };
    if renew_over_session(socket, r2).await?
        != expected_renewal(
            database,
            15_300_000,
            expected_lease_status(database, 1, PEER, LeaseState::Held, 15_300_000, 14_400_000)?,
            false,
        )?
    {
        return Err(
            "fresh R2 did not extend the actual same-epoch lease by fifteen minutes".into(),
        );
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let route = format!("{base}/admin/v1/projects/{database}/takeover");
    let preview = owner_registry_scenarios::post(
        &http,
        &format!("{route}/preview"),
        &serde_json::Value::Null,
    )
    .await?;
    let preview: TakeoverPreview = serde_json::from_value(
        preview
            .get("preview")
            .cloned()
            .ok_or("renewal takeover omitted native preview")?,
    )?;
    if preview.fence != Epoch::new(1)?
        || preview.acked != original_prefix()?
        || preview.holder != PeerId::try_from(PEER.to_owned())?
    {
        return Err("renewal supersession preview lost the actual old holder/prefix".into());
    }
    let takeover = json!({"transferId":Uuid::now_v7(),"toInstallation":TARGET_SERVICE,"reason":"renewal supersession proof","preview":preview});
    let initial = owner_registry_scenarios::post(&http, &route, &takeover).await?;
    let granted = owner_registry_scenarios::complete(&http, &route, &takeover, initial).await?;
    if granted != json!({"kind":"granted","value":{"newEpoch":2,"start":{"epoch":1,"sequence":1}}})
    {
        return Err(format!(
            "normal supersession did not grant epoch2 at the original prefix: {granted}"
        )
        .into());
    }
    if owner_registry_scenarios::post(&http, &route, &takeover).await? != granted {
        return Err("renewal supersession takeover did not replay its original grant".into());
    }
    println!("RENEWAL_HISTORY_STAGE actual_owner_takeover_epoch2 passed");
    // Assert R1's original success together with current authority belonging to the target.
    let expected = expected_renewal(
        database,
        14_400_000,
        expected_lease_status(
            database,
            2,
            TARGET_PEER,
            LeaseState::Held,
            15_300_000,
            14_400_000,
        )?,
        true,
    )?;
    if renew_over_session(socket, r1).await? != expected {
        return Err("R1 replay after takeover confused its recorded success with current superseding authority".into());
    }
    let fresh = RenewRequest {
        attempt: RequestId::new(Uuid::now_v7())?,
        database,
        epoch: Epoch::new(1)?,
    };
    if renew_over_session(socket, fresh).await?
        != RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
            current_epoch: Epoch::new(2)?,
            epoch_ended_at: original_prefix()?,
        })
    {
        return Err(
            "fresh old-holder renewal lost the exact superseding epoch/cutoff refusal".into(),
        );
    }
    snapshot_unchanged(socket, database).await?;
    println!(
        "RENEWAL_HISTORY_CASE lost_R1_fresh_R2_and_recorded_success_with_superseding_authority passed"
    );
    Ok(())
}
async fn expired_welcome_and_same_epoch_renewal(
    base: &str,
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    pin_registry(base, 18_000_000).await?;
    println!("RENEWAL_HISTORY_STAGE registry_clock_five_hours_advanced passed");
    let welcome = reconnect_writer(base, socket).await?;
    if welcome
        != vec![DatabaseStatus::Ready {
            database,
            acked: original_prefix()?,
            lease: expected_lease_status(database, 1, PEER, LeaseState::Expired, 14_400_000, 0)?,
        }]
    {
        return Err(format!(
            "five-hour Welcome did not report the exact expired original lease: {welcome:?}"
        )
        .into());
    }
    println!("RENEWAL_HISTORY_STAGE expired_Welcome_at_five_hours passed");
    let request = RenewRequest {
        attempt: RequestId::new(Uuid::now_v7())?,
        database,
        epoch: Epoch::new(1)?,
    };
    if renew_over_session(socket, request).await?
        != expected_renewal(
            database,
            32_400_000,
            expected_lease_status(database, 1, PEER, LeaseState::Held, 32_400_000, 14_400_000)?,
            false,
        )?
    {
        return Err("fresh renewal did not restore Held in the same expired epoch".into());
    }
    snapshot_unchanged(socket, database).await?;
    println!("RENEWAL_HISTORY_CASE expired_lease_renews_same_epoch passed");
    Ok(())
}
async fn run_normal_case(base: &str, case: RenewalCase) -> ConformanceResult<()> {
    pin_registry(base, 0).await?;
    let client = ConformanceClient::new(base)?;
    let mut writer = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let target = enroll_admitted_peer(
        &client,
        TARGET_SERVICE,
        TARGET_PEER,
        "renewal history target",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof=async {
        let database=DatabaseId::new(Uuid::now_v7())?;let workspace=WorkspaceId::generate();let board=BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("renewal history workspace".to_owned())?,description:Description::try_from("recorded and current authority".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("renewal history board".to_owned())?,description:Description::try_from("normal registered renewal".to_owned())?,board_kind:message_board::BoardKind::Area {}},
            creation::project(database,board)?,
        ] {creation::accepted(creation::metadata(&mut writer.socket,&workspace,operation).await?)?;}
        creation::registered(&creation::register(&mut writer.socket,database,PeerId::try_from(PEER.to_owned())?,RequestId::new(Uuid::now_v7())?).await?,PeerId::try_from(PEER.to_owned())?)?;
        let piece=Piece::<BoardAnnotation>::new(PieceContents {database,epoch:Epoch::new(1)?,continues_from:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(0)),through:original_prefix()?,transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE renewal_records(id TEXT NOT NULL PRIMARY KEY)".into()]}],part:None,annotation:None})?;
        if !matches!(creation::operation_answer(&mut writer.socket,OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==original_prefix()?) {return Err("renewal fixture did not commit its normal schema1 prefix".into());}
        match case {RenewalCase::Supersession=>lost_replay_fresh_extension_and_supersession(base,&mut writer.socket,database).await,RenewalCase::Expiry=>expired_welcome_and_same_epoch_renewal(base,&mut writer.socket,database).await}
    }.await;
    let old_closed = writer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let target_closed = target
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    old_closed?;
    target_closed?;
    Ok(())
}
async fn run_native_case(case: RenewalCase) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = run_normal_case(server.base_url(), case).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}
#[tokio::test]
async fn lost_renewal_replay_fresh_extension_and_takeover_keep_recorded_and_current_separate()
-> ConformanceResult<()> {
    run_native_case(RenewalCase::Supersession).await
}
#[tokio::test]
async fn expired_welcome_is_restored_by_a_fresh_same_epoch_renewal() -> ConformanceResult<()> {
    run_native_case(RenewalCase::Expiry).await
}
