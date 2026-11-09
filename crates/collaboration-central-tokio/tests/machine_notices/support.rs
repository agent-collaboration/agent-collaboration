use super::*;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, storage::TransactionScope};
use fleet_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{query::query, row::Row};
use std::time::Duration;
use tokio::sync::mpsc;
pub const RETENTION: i64 = 7 * 24 * 60 * 60 * 1_000;
type Socket = mpsc::Receiver<SocketCommand>;
pub async fn insert_zero_position(fixture: &Fixture) -> Result<(), TestError> {
    let id = durable_steps::Uuid::now_v7().to_string();
    let created = fixture.io.now().unix_millis();
    fixture.host.store.object.transaction(move |scope|Box::pin(async move {
        query::<sqlx_turso::Turso>("INSERT INTO machine_control_project_notices(notice_id,machine_id,position,kind,created_at,expires_at,updated_at) SELECT ?,machine_id,0,'project_list_changed',?,?,? FROM machine_enrollments LIMIT 1")
            .bind(id).bind(created).bind(created+RETENTION).bind(created).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(())
}
pub async fn state_rows(fixture: &Fixture) -> Result<i64, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT COUNT(*) AS count FROM machine_control_project_progress",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get("count").map_err(StoreError::backend)
            })
        })
        .await?)
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
pub fn snapshot_closed_owner(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), TestError> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_str().ok_or("non-UTF8 native owner filename")?;
        if name_text == "fleet.db" || name_text.starts_with("fleet.db-") {
            assert!(entry.file_type()?.is_file());
            eprintln!(
                "closed owner snapshot: {name_text}, {} bytes",
                entry.metadata()?.len()
            );
            std::fs::copy(entry.path(), destination.join(name))?;
        }
    }
    assert!(destination.join("fleet.db").is_file());
    Ok(())
}
/// The hint a Fleet metadata mutation appends to a feed reader's machine.
/// Positions are the stream's own; `position` names the one a caller expects.
pub fn notice(_position: u64) -> Result<BoardNotice, TestError> {
    Ok(BoardNotice::ProjectListChanged {})
}

const READER: &str = "019f0000-0000-7000-8000-000000000001";

fn fleet_call(kind: &str, request: serde_json::Value) -> Result<Vec<u8>, TestError> {
    Ok(serde_json::to_vec(&serde_json::json!({
        "source":{"kind":"fleet"},"sourceFace":"session",
        "authority":{"kind":"machine","value":{"installation":READER}},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"component_call"},
        "kind":kind,"target":null,"request":request
    }))?)
}

/// A Fleet metadata mutation: the real producer of a feed reader's notice.
/// The kind is ignored; every mutation appends a project-list hint.
pub fn invocation(_kind: &BoardNotice) -> Result<Vec<u8>, TestError> {
    let request = fleet_admission::FleetRequest {
        request_id: message_board::RequestId::generate(),
        // A fresh workspace and name each time: every create is a new committed change.
        workspace: durable_steps::Uuid::now_v7().to_string().try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "notice producer".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: fleet_admission::FleetOperation::WorkspaceCreate {
            name: format!("notice {}", durable_steps::Uuid::now_v7()).try_into()?,
            description: "appends one project-list hint".to_owned().try_into()?,
        },
    };
    fleet_call(
        "central.fleet-metadata",
        serde_json::json!({"origin":{"installation":READER,"machine":"notice fixture"},"request":request}),
    )
}

async fn call_event(fixture: &Fixture, bytes: Vec<u8>) -> Result<Vec<u8>, TestError> {
    match collaboration_central_core::ObjectKernel::new(&fixture.runtime)
        .on_event(collaboration_central_core::ObjectEvent::Call {
            body: bytes,
            storage_bytes: None,
        })
        .await
    {
        collaboration_central_core::EventReply::CallAnswer(answer) => Ok(answer),
        other => Err(format!("Fleet call answered {other:?}").into()),
    }
}

/// Append through the real producer, as a kernel event, so the event's
/// post-event push sends the hint. The reader's installation registers as a
/// feed reader first (idempotent).
pub async fn deliver(fixture: &Fixture, bytes: &[u8]) -> Result<(), TestError> {
    call_event(
        fixture,
        fleet_call(
            "central.fleet-read",
            serde_json::json!({"kind":"feed","value":{"after":0}}),
        )?,
    )
    .await?;
    let answer: fleet_admission::FleetRequestResult =
        serde_json::from_slice(&call_event(fixture, bytes.to_vec()).await?)?;
    assert!(
        matches!(
            &answer,
            fleet_admission::FleetRequestResult::Decided(
                message_board::RequestDecision::Accepted { .. }
            )
        ),
        "the notice-producing mutation was not accepted: {answer:?}"
    );
    Ok(())
}
pub async fn admit(
    fixture: &Fixture,
    peer: PeerId,
) -> Result<(ConnectionBinding, SessionId, Socket), TestError> {
    fixture.enroll(peer).await?;
    let binding = ConnectionBinding::new(
        ConnectionId::new(durable_steps::Uuid::now_v7())?,
        VerifiedPeer {
            peer,
            expires_at: SyncTime::from_unix_millis(
                fixture
                    .io
                    .now()
                    .unix_millis()
                    .checked_add(3_600_000)
                    .ok_or("credential expiry overflow")?,
            ),
        },
    );
    let origin = collaboration_central_core::AdmissionOrigin {
        installation: message_board::ServiceId::try_from(
            "019f0000-0000-7000-8000-000000000001".to_owned(),
        )?,
        machine: "notice fixture".to_owned().try_into()?,
    };
    let mut socket = fixture.host.native.attach_socket(binding, origin).await;
    fixture.runtime.on_socket_opened(binding).await?;
    let session = SessionId::new(durable_steps::Uuid::now_v7())?;
    fixture.hello(binding, session).await?;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert!(
        matches!(next(&mut socket).await?,ServerFrame::Welcome {session_id,..} if session_id==session)
    );
    Ok((binding, session, socket))
}
pub async fn next(socket: &mut Socket) -> Result<ServerFrame<BoardConsumerProtocol>, TestError> {
    let Some(SocketCommand::Frame(bytes)) =
        tokio::time::timeout(Duration::from_secs(5), socket.recv()).await?
    else {
        return Err("expected a committed session frame".into());
    };
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn read(
    fixture: &Fixture,
    binding: ConnectionBinding,
    socket: &mut Socket,
    after: Option<u64>,
) -> Result<NoticeRead, TestError> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .runtime
        .on_frame(
            binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope: BoardReadScope::Notices,
                    cursor: after
                        .map(|position| BoardReadCursor::Notices(NoticePosition::new(position))),
                }),
            })?,
        )
        .await?;
    let ServerFrame::Answer {
        frame: actual,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(read))),
    } = next(socket).await?
    else {
        return Err("notice read did not yield exactly its typed answer".into());
    };
    assert_eq!(actual, frame);
    Ok(read)
}
#[expect(
    clippy::panic,
    reason = "The test oracle must reject a different notice-read outcome."
)]
pub fn assert_page(read: NoticeRead, expected: &[(u64, BoardNotice)]) {
    let NoticeRead::Page { notices, .. } = read else {
        panic!("expected retained notice page: {read:?}")
    };
    let expected = expected
        .iter()
        .map(|(position, kind)| PositionedBoardNotice {
            position: NoticePosition::new(*position),
            kind: kind.clone(),
        })
        .collect::<Vec<_>>();
    assert_eq!(notices, expected);
}
pub fn assert_hint(
    frame: ServerFrame<BoardConsumerProtocol>,
    transport: u64,
    machine: u64,
    expected: &BoardNotice,
) {
    assert!(
        matches!(frame,ServerFrame::Notice {position,body:Notice::Consumer(PositionedBoardNotice {position:machine_position,kind})}
        if position.value()==transport && machine_position.value()==machine && &kind==expected)
    );
}
pub async fn rows(fixture: &Fixture) -> Result<i64, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT COUNT(*) AS count FROM machine_control_project_notices",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get("count").map_err(StoreError::backend)
            })
        })
        .await?)
}
