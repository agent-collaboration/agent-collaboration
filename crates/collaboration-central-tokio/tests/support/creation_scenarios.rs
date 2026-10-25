//! Normal enrollment, Fleet decisions and Registry registration share no gate stand-in.
use board_sync_protocol::{
    BoardAnnotation, BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest,
    BoardPayload, FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ClockObservation, ConformanceClient, ConformanceResult, RuntimeControl, RuntimeControlRequest,
    RuntimeFaultObservation, enroll_admitted_peer, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, DatabaseId, Epoch, FrameId, LeaseAnswer, LeaseRequest,
    OperationAnswer, OperationRequest, Origin, PeerId, Piece, PieceContents, Registration,
    RegistrationAnswer, RegistrationRefusal, ReplicatedTransaction, ReplicationAnswer,
    ReplicationPosition, ReplicationRequest, RequestId, ServerFrame,
};
use futures_util::{SinkExt, StreamExt};
use message_board::{
    BoardId, Description, HumanId, ProjectId, RecordedActor, RequestDecision, ResourceName,
    WorkspaceId,
};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440469";
const PEER: &str = "019f0000-0000-7000-8000-000000000469";
const OTHER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440470";
const OTHER_PEER: &str = "019f0000-0000-7000-8000-000000000470";

pub(super) async fn run(base_url: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base_url)?;
    let mut creator = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        "creation-carrier",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let mut other = enroll_admitted_peer(
        &client,
        OTHER_SERVICE,
        OTHER_PEER,
        "other-creation-carrier",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof=async {
        let now=RegistryTime("2026-10-20T00:00:00Z".parse()?);
        pin_registry_clock(base_url,now).await?;
        let workspace=WorkspaceId::try_from(Uuid::now_v7().to_string())?;
        let board=BoardId::try_from(Uuid::now_v7().to_string())?;
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("normal creation gates".to_owned())?,description:Description::try_from("real Fleet and Registry".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("normal creation board".to_owned())?,description:Description::try_from("actual parents".to_owned())?,board_kind:message_board::BoardKind::Area {}},
        ] {
            accepted(metadata(&mut creator.socket,&workspace,operation).await?)?;
        }
        let database=DatabaseId::new(Uuid::now_v7())?;
        accepted(metadata(&mut creator.socket,&workspace,project(database,board.clone())?).await?)?;
        let wrong=register(&mut other.socket,database,PeerId::new(other.peer)?,RequestId::new(Uuid::now_v7())?).await?;
        match wrong {
            RegistrationAnswer::Refused(RegistrationRefusal::DatabaseRejected {reason})
                if reason.code.as_str()=="creatorMismatch" && reason.detail=="The project was created by another installation."=>{},
            _=>return Err("another enrolled installation was not refused by the stored carrier gate".into()),
        }
        println!("CREATION_GATE_CASE accepted_other_carrier_refused passed");
        let attempt=RequestId::new(Uuid::now_v7())?;
        let first=register(&mut creator.socket,database,PeerId::new(creator.peer)?,attempt).await?;
        registered(&first,PeerId::new(creator.peer)?)?;
        let replay=register(&mut creator.socket,database,PeerId::new(creator.peer)?,attempt).await?;
        if replay != first {return Err("normal registration retry changed its original lease answer".into());}
        println!("CREATION_GATE_CASE accepted_carrier_registered_and_replayed passed");
        let RegistrationAnswer::Registered { lease, .. } = &first else {
            return Err("registration lost its original lease".into());
        };
        await_registration_publication(&mut creator.socket, database, lease.expires_at).await?;
        println!("CREATION_PUBLICATION_CASE normal_registration_reaches_directory_without_forced_alarm passed");
        let epoch=Epoch::new(1)?;
        let through=ReplicationPosition::new(epoch,CommitSequence::new(1));
        let first_piece=Piece::<BoardAnnotation>::new(PieceContents {
            database,epoch,continues_from:ReplicationPosition::new(epoch,CommitSequence::new(0)),through,
            transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE records(id TEXT NOT NULL PRIMARY KEY,label TEXT NOT NULL)".to_owned()]}],part:None,annotation:None,
        })?;
        if !matches!(operation_answer(&mut creator.socket,OperationRequest::Replication(ReplicationRequest::Piece(first_piece))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through:actual}) if actual==through) {
            return Err("normal Fleet then Register did not admit the first real Copy piece".into());
        }
        println!("CREATION_PIECE_CASE registered_first_epoch_accepts_its_first_piece passed");
        match operation_answer(&mut other.socket,OperationRequest::Lease(LeaseRequest::Claim {
            attempt:RequestId::new(Uuid::now_v7())?,database,
        })).await? {
            OperationAnswer::Lease(LeaseAnswer::Claim(fleet_sync_protocol::HolderChangeAnswer::Refused(
                fleet_sync_protocol::HolderChangeRefusal::AlreadyHeld {holder},
            ))) if holder==PeerId::new(creator.peer)?=>{},
            _=>return Err("normal Claim did not preserve the actual existing holder".into()),
        }
        println!("CREATION_CLAIM_CASE registered_database_keeps_its_holder passed");
        let unknown = DatabaseId::new(Uuid::now_v7())?;
        let unknown_attempt = RequestId::new(Uuid::now_v7())?;
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match operation_answer(&mut other.socket, OperationRequest::Lease(LeaseRequest::Claim {
                        attempt: unknown_attempt, database: unknown,
                    })).await? {
                        OperationAnswer::Lease(LeaseAnswer::Claim(fleet_sync_protocol::HolderChangeAnswer::Refused(
                            fleet_sync_protocol::HolderChangeRefusal::NotFound,
                        ))) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()),
                        OperationAnswer::Lease(LeaseAnswer::Claim(fleet_sync_protocol::HolderChangeAnswer::Pending { transfer }))
                            if transfer.as_uuid() == unknown_attempt.as_uuid() => tokio::task::yield_now().await,
                        _ => return Err("unknown Claim did not preserve its correlated NotFound refusal".into()),
                    }
                }
            }).await??;
        }
        println!("CREATION_CLAIM_CASE unknown_database_and_same_attempt_replay_are_not_found passed");

        let rejected=DatabaseId::new(Uuid::now_v7())?;
        let missing_board=BoardId::try_from(Uuid::now_v7().to_string())?;
        let decision=metadata(&mut creator.socket,&workspace,project(rejected,missing_board.clone())?).await?;
        if !matches!(decision,FleetRequestResult::Decided(RequestDecision::Rejected {reasons}) if reasons==vec![message_board::Refusal::NotFound(missing_board.clone().into())]) {
            return Err("rejected creation fixture did not persist its exact NotFound".into());
        }
        match register(&mut creator.socket,rejected,PeerId::new(creator.peer)?,RequestId::new(Uuid::now_v7())?).await? {
            RegistrationAnswer::Refused(RegistrationRefusal::DatabaseRejected {reason})
                if reason.code.as_str()=="notFound" && reason.detail==format!("Record {} was not found.",missing_board.as_str())=>{},
            _=>return Err("rejected creation did not carry its exact code and rendered detail into registration".into()),
        }
        println!("CREATION_GATE_CASE rejected_decision_preserves_reason passed");

        let later=DatabaseId::new(Uuid::now_v7())?;
        let attempt=RequestId::new(Uuid::now_v7())?;
        send(&mut other.socket,OperationRequest::Lease(LeaseRequest::Register(Registration {
            attempt,database:later,writer:PeerId::new(other.peer)?,origin:Origin::Empty,
        }))).await?;
        // Registration has no Pending wire variant. The unavailable answer
        // closes this transport; the durable attempt must remain retryable.
        match tokio::time::timeout(Duration::from_secs(5),other.socket.next()).await? {
            None | Some(Ok(Message::Close {..}))=>{},
            other=>return Err(format!("missing creation returned a terminal answer instead of retryable transport loss: {other:?}").into()),
        }
        // The existing helper's exact Welcome oracle requires no held
        // databases. The other installation owns no lease after either refusal.
        let mut reopened=enroll_admitted_peer(&client,OTHER_SERVICE,OTHER_PEER,"other-creation-carrier",&Uuid::now_v7().to_string()).await?;
        let proof=async {
            accepted(metadata(&mut reopened.socket,&workspace,project(later,board)?).await?)?;
            // Advance the owning clock rather than waiting or polling for the
            // durable call's retry backoff. The normal retry drives due work.
            pin_registry_clock(base_url,RegistryTime("2026-10-20T00:01:00Z".parse()?)).await?;
            let answer=register(&mut reopened.socket,later,PeerId::new(reopened.peer)?,attempt).await?;
            registered(&answer,PeerId::new(reopened.peer)?)?;
            println!("CREATION_GATE_CASE absent_decision_retries_same_attempt_after_reconnect passed");
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let closed=reopened.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof?;closed?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = creator
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let _ = other
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

pub(super) fn project(database: DatabaseId, board: BoardId) -> ConformanceResult<FleetOperation> {
    Ok(FleetOperation::ProjectDirectoryCreate {
        project: ProjectId::try_from(database.to_string())?,
        board,
        title: ResourceName::try_from(format!("creation gate {database}"))?,
    })
}
pub(super) fn accepted(answer: FleetRequestResult) -> ConformanceResult<()> {
    if !matches!(
        answer,
        FleetRequestResult::Decided(RequestDecision::Accepted { .. })
    ) {
        return Err(format!(
            "normal creation metadata was not accepted: {}",
            serde_json::to_string(&answer)?
        )
        .into());
    }
    Ok(())
}
pub(super) fn registered(answer: &RegistrationAnswer, writer: PeerId) -> ConformanceResult<()> {
    if !matches!(answer,RegistrationAnswer::Registered {lease,..} if lease.holder==writer && lease.epoch.value()==1 && lease.epoch==Epoch::new(1)?)
    {
        return Err("normal registration did not grant epoch1 to the authenticated writer".into());
    }
    Ok(())
}
pub(super) async fn metadata(
    socket: &mut WebSocket,
    workspace: &WorkspaceId,
    operation: FleetOperation,
) -> ConformanceResult<FleetRequestResult> {
    let request = FleetRequest {
        request_id: message_board::RequestId::try_from(Uuid::now_v7().to_string())?,
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: HumanId::try_from(
                "declared metadata actor is not carrier authority".to_owned(),
            )?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation,
    };
    match operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request.request_id.clone(),
            operation: BoardPayload::encode(&request)?,
        })),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) if request_id == request.request_id => Ok(decision.decode()?),
        _ => Err("normal creation metadata lost its typed answer or request identity".into()),
    }
}
pub(super) async fn register(
    socket: &mut WebSocket,
    database: DatabaseId,
    writer: PeerId,
    attempt: RequestId,
) -> ConformanceResult<RegistrationAnswer> {
    match operation_answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Register(Registration {
            attempt,
            database,
            writer,
            origin: Origin::Empty,
        })),
    )
    .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Register(answer)) => Ok(answer),
        _ => Err("normal registration returned another operation family".into()),
    }
}
pub(super) async fn pin_registry_clock(base_url: &str, now: RegistryTime) -> ConformanceResult<()> {
    let answer: RuntimeFaultObservation = reqwest::Client::new()
        .post(format!("{base_url}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Registry,
            command: RuntimeControl::SetTime { now },
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !matches!(answer.clock,ClockObservation::Pinned {now:actual} if actual==now) {
        return Err("Registry clock control did not reach its owning object".into());
    }
    Ok(())
}
pub(super) async fn send(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<FrameId> {
    let frame = FrameId::new(Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    Ok(frame)
}
pub(super) async fn operation_answer(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = send(socket, body).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("creation session ended before answer")??;
            let bytes = match message {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                other => return Err(format!("creation request received {other:?}").into()),
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body,
                } if actual == frame => return Ok(body),
                ServerFrame::Notice { .. } => {}
                _ => return Err("creation request lost its exact frame correlation".into()),
            }
        }
    })
    .await?
}

async fn await_registration_publication(
    socket: &mut WebSocket,
    database: DatabaseId,
    expected_expiry: fleet_sync_protocol::SyncTime,
) -> ConformanceResult<()> {
    use board_sync_protocol::{BoardReadPage, BoardReadScope};
    use fleet_admission::{
        DirectoryEntry, DirectoryLease, DirectoryQuery, DirectorySide, LeaseMetadata,
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
                entries,
                ..
            })) = operation_answer(
                socket,
                OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope: BoardReadScope::Directory {
                        query: BoardPayload::encode(&DirectoryQuery::default())?,
                    },
                    cursor: None,
                }),
            )
            .await?
            else {
                return Err("normal registration publication read returned another family".into());
            };
            let entries = entries
                .into_iter()
                .map(|entry| entry.decode::<DirectoryEntry>())
                .collect::<Result<Vec<_>, _>>()?;
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("normal registration publication omitted the project")?;
            match &entry.lease_side {
                DirectorySide::NotYetReported => tokio::task::yield_now().await,
                DirectorySide::Known(LeaseMetadata {
                    lease:
                        DirectoryLease::Held {
                            holder, expires_at, ..
                        },
                    ..
                }) if holder.as_str() == SERVICE
                    && expires_at.timestamp_millis() == expected_expiry.unix_millis() =>
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                other => {
                    return Err(format!(
                        "normal registration published another holder or lease: {other:?}"
                    )
                    .into());
                }
            }
        }
    })
    .await??;
    Ok(())
}
