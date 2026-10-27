use super::*;
use collaboration_central_tokio::SocketCommand;
use durable_steps::{AtomicStore, storage::TransactionScope};
use fleet_sync_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{query::query, row::Row};
use std::time::Duration;
use tokio::sync::mpsc;
pub const RETENTION: i64 = 7 * 24 * 60 * 60 * 1_000;
type Socket = mpsc::Receiver<SocketCommand>;
pub async fn insert_zero_position(fixture: &Fixture) -> Result<(), TestError> {
    let id = durable_steps::Uuid::now_v7().to_string();
    let kind = serde_json::to_string(&notice(1)?)?;
    let created = fixture.io.now().unix_millis();
    fixture.host.store.object.transaction(move |scope|Box::pin(async move {
        query::<sqlx_turso::Turso>("INSERT INTO machine_notices(notice_id,position,kind,created_at,expires_at) VALUES(?,0,?,?,?)")
            .bind(id).bind(kind).bind(created).bind(created+RETENTION).execute(scope.executor()).await.map_err(StoreError::backend)?;
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
                    "SELECT COUNT(*) AS count FROM machine_notice_state",
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
        if name_text == "session.db" || name_text.starts_with("session.db-") {
            assert!(entry.file_type()?.is_file());
            eprintln!(
                "closed owner snapshot: {name_text}, {} bytes",
                entry.metadata()?.len()
            );
            std::fs::copy(entry.path(), destination.join(name))?;
        }
    }
    assert!(destination.join("session.db").is_file());
    Ok(())
}
pub fn notice(through: u64) -> Result<BoardNotice, TestError> {
    Ok(BoardNotice::FleetFeedAdvanced {
        through: message_board::FleetPosition::try_from(through)?,
    })
}
pub fn invocation(kind: &BoardNotice) -> Result<Vec<u8>, TestError> {
    Ok(serde_json::to_vec(&serde_json::json!({
        "source":{"kind":"fleet"},"sourceFace":"fleet","authority":{"kind":"object"},
        "destination":{"kind":"connection","value":"019f0000-0000-7000-8000-000000000001"},"destinationFace":"delivery",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"deliver_machine_notice"},
        "kind":"central.machine-notice","target":{"installation":"019f0000-0000-7000-8000-000000000001"},"request":kind
    }))?)
}
pub async fn deliver(fixture: &Fixture, bytes: &[u8]) -> Result<(), TestError> {
    assert!(
        matches!(
            fixture.runtime.delivery()?.on_call(bytes.to_vec()).await,
            Err(durable_steps::CallFailure::AnswerLost)
        ),
        "a newly admitted operation cannot acknowledge before its append commits"
    );
    fixture.runtime.on_alarm().await?;
    assert_eq!(
        fixture.runtime.delivery()?.on_call(bytes.to_vec()).await?,
        b"null"
    );
    Ok(())
}
pub async fn admit(
    fixture: &Fixture,
    peer: PeerId,
) -> Result<(ConnectionBinding, SessionId, Socket), TestError> {
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
    fixture.runtime.on_alarm().await?;
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
                let row =
                    query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM machine_notices")
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                row.try_get("count").map_err(StoreError::backend)
            })
        })
        .await?)
}
pub async fn stored_peer(fixture: &Fixture) -> Result<String, TestError> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>("SELECT peer_id FROM machine_notice_state")
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                row.try_get("peer_id").map_err(StoreError::backend)
            })
        })
        .await?)
}
