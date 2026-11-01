//! A lost real writer answer is retried by the real central caller journal.

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion,
};
use durable_steps::OperationId;
use fleet_sync_lease::{LeaseHolder, LeasePolicy, WriterState};
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DatabaseId, Epoch, FrameId, LeaseState, LeaseStatus, NoConsumer,
    PeerId, ReleaseAnswer, ReleaseRefusal, ReleaseRequest, ServerCall, ServerCallAnswer,
    ServerFrame, ServerRequest, TransferId, WithdrawAnswer, WithdrawRelease,
};
use fleet_sync_writer::{
    Migration, ReplicatedDatabase, ReplicatedSchema, WriteRefusal, WriterOptions,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use sqlx_core::{
    executor::Executor, migrate::MigrationType, query::query, row::Row, sql_str::SqlSafeStr,
};
use std::{
    path::Path,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::task::JoinSet;

pub(super) const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400c3";
pub(super) const PEER: &str = "019f0000-0000-7000-8000-000000001024";
pub(super) const CREDENTIAL: &str = "holder-real-writer";
pub(super) const DATABASE: &str = "019f0000-0000-7000-8000-000000000703";
pub(super) const INITIAL_SESSION: &str = "019f0000-0000-7000-8000-000000000701";
pub(super) const RECONNECTED_SESSION: &str = "019f0000-0000-7000-8000-000000000702";
static MIGRATIONS: LazyLock<Vec<Migration>> = LazyLock::new(|| {
    vec![Migration::new(
        1,
        "holder records".into(),
        MigrationType::Simple,
        include_str!("../fixtures/holder_writer/0001_records.sql").into_sql_str(),
        false,
    )]
});
static TABLES: LazyLock<
    Result<Vec<fleet_sync_protocol::TableName>, fleet_sync_protocol::TextValueError>,
> = LazyLock::new(|| {
    ["holder_records"]
        .into_iter()
        .map(fleet_sync_protocol::TableName::new)
        .collect()
});

pub(super) struct HolderSchema {
    tables: &'static [fleet_sync_protocol::TableName],
}
impl ReplicatedSchema for HolderSchema {
    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS.as_slice()
    }
    fn replicated_tables(&self) -> &'static [fleet_sync_protocol::TableName] {
        self.tables
    }
}
struct WriterClock;
impl Clock for WriterClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_900_000_000_000)
    }
}
struct HolderConnection {
    peer: PeerId,
    socket: Option<WebSocket>,
}
impl HolderConnection {
    fn socket(&mut self) -> ConformanceResult<&mut WebSocket> {
        self.socket
            .as_mut()
            .ok_or("holder connection is closed".into())
    }
    async fn close(&mut self) -> ConformanceResult<()> {
        if let Some(socket) = self.socket.take() {
            socket
                .close(reqwest_websocket::CloseCode::Normal, None)
                .await?;
        }
        Ok(())
    }
    async fn reconnect(&mut self, client: &ConformanceClient) -> ConformanceResult<()> {
        let admitted =
            enroll_admitted_peer(client, SERVICE, PEER, CREDENTIAL, RECONNECTED_SESSION).await?;
        if PeerId::new(admitted.peer)? != self.peer {
            admitted
                .socket
                .close(reqwest_websocket::CloseCode::Normal, None)
                .await?;
            return Err("reconnected holder changed its stable peer".into());
        }
        self.socket = Some(admitted.socket);
        Ok(())
    }
}
pub(super) async fn writer(
    path: &Path,
    peer: PeerId,
) -> ConformanceResult<ReplicatedDatabase<HolderSchema>> {
    let database = DatabaseId::try_from(DATABASE.to_owned())?;
    let policy = LeasePolicy {
        lease_duration: Duration::from_secs(14_400),
        renew_interval: Duration::from_secs(900),
    };
    let mut holder = LeaseHolder::provisional(database, peer, policy);
    // An explicit grant fixture isolates holder retry from the not-yet-composed
    // registration/copy workflow. The real release receipts and file stay real.
    holder.observe_registration(
        LeaseStatus {
            database,
            epoch: Epoch::new(1)?,
            holder: peer,
            state: LeaseState::Held,
            expires_at: SyncTime::from_unix_millis(2_000_000_000_000),
            remaining_ms: 14_400_000,
        },
        WriterClock.now(),
    )?;
    Ok(ReplicatedDatabase::open_with(
        path,
        database,
        HolderSchema {
            tables: TABLES.as_ref().map_err(Clone::clone)?,
        },
        holder,
        Arc::new(WriterClock),
        WriterOptions::default(),
    )
    .await?)
}
pub(super) async fn amount(database: &ReplicatedDatabase<HolderSchema>) -> ConformanceResult<i64> {
    Ok(query::<sqlx_turso::Turso>("SELECT amount FROM holder_records WHERE record_id = '019f0000-0000-7000-8000-000000000704'").fetch_one(database.reader()).await?.try_get("amount")?)
}
pub(crate) async fn owner_post(
    http: &reqwest::Client,
    base: &str,
    route: &str,
    body: Value,
) -> ConformanceResult<Value> {
    Ok(http
        .post(format!("{base}/admin/v1/conformance/{route}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
pub(crate) fn start(
    http: &reqwest::Client,
    base: &str,
    tasks: &mut JoinSet<ConformanceResult<Value>>,
    input: Value,
) {
    let http = http.clone();
    let base = base.to_owned();
    tasks.spawn(async move { owner_post(&http, &base, "holder-call", input).await });
}
pub(crate) async fn pending_reply(
    tasks: &mut JoinSet<ConformanceResult<Value>>,
) -> ConformanceResult<()> {
    let reply = tokio::time::timeout(Duration::from_secs(10), tasks.join_next())
        .await?
        .ok_or("caller RPC disappeared")???;
    if reply.get("kind").and_then(Value::as_str) != Some("pending") {
        return Err(
            format!("lost holder answer produced a terminal acknowledgement: {reply}").into(),
        );
    }
    Ok(())
}
pub(crate) async fn read_status(
    http: &reqwest::Client,
    base: &str,
    operation: OperationId,
) -> ConformanceResult<Value> {
    owner_post(
        http,
        base,
        "holder-status",
        serde_json::to_value(operation)?,
    )
    .await
}
async fn uncertain(
    http: &reqwest::Client,
    base: &str,
    operation: OperationId,
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let status = read_status(http, base, operation).await?;
            if status
                .pointer("/value/step/state/attempt/reach")
                .and_then(Value::as_str)
                == Some("may_have_reached")
            {
                if status
                    .pointer("/value/step/state/intent/replay")
                    .and_then(Value::as_str)
                    != Some("deduplicated")
                {
                    return Err("caller did not retain deduplicated replay semantics".into());
                }
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if status.get("kind").and_then(Value::as_str) != Some("pending") {
                return Err(format!("missing uncertain call in the real journal: {status}").into());
            }
            // This endpoint offers only reads. Backoff never establishes
            // readiness; the next authoritative journal read does.
        }
    })
    .await??;
    Ok(())
}
async fn completed(
    http: &reqwest::Client,
    base: &str,
    operation: OperationId,
    kind: &str,
    answer: Value,
) -> ConformanceResult<()> {
    let expected = json!({"kind":"completed","value":{"kind":kind,"value":answer}});
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = read_status(http, base, operation).await?;
            if status == expected {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if status.get("kind").and_then(Value::as_str) != Some("pending") {
                return Err(format!("writer answer did not complete its caller: {status}").into());
            }
        }
    })
    .await??;
    Ok(())
}
pub(crate) async fn received(socket: &mut WebSocket) -> ConformanceResult<(FrameId, ServerCall)> {
    received_within(socket, Duration::from_secs(8)).await
}

pub(crate) async fn received_within(
    socket: &mut WebSocket,
    bound: Duration,
) -> ConformanceResult<(FrameId, ServerCall)> {
    let message = tokio::time::timeout(bound, socket.next())
        .await?
        .ok_or("holder socket ended")??;
    let bytes = match message {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => return Err(format!("unexpected holder frame: {other:?}").into()),
    };
    match serde_json::from_slice::<ServerFrame<NoConsumer>>(&bytes)? {
        ServerFrame::Request {
            frame,
            body: ServerRequest::Crate(call),
        } => Ok((frame, call)),
        _ => Err("expected the real caller's holder request".into()),
    }
}
pub(crate) async fn send(
    socket: &mut WebSocket,
    frame: FrameId,
    answer: ServerCallAnswer,
) -> ConformanceResult<()> {
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<NoConsumer>::Answer {
                frame,
                body: ClientAnswer::Crate(answer),
            })?
            .into(),
        ))
        .await?;
    Ok(())
}

pub async fn run(base: &str, path: &Path) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let admitted =
        enroll_admitted_peer(&client, SERVICE, PEER, CREDENTIAL, INITIAL_SESSION).await?;
    let identity = PeerId::new(admitted.peer)?;
    let mut peer = HolderConnection {
        peer: identity,
        socket: Some(admitted.socket),
    };
    let mut database = writer(path, identity).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut tasks = JoinSet::new();
    let result = cases(
        base,
        path,
        &client,
        &http,
        &mut peer,
        &mut database,
        &mut tasks,
    )
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    database.close().await;
    let closed = peer.close().await;
    result?;
    closed?;
    println!(
        "HOLDER_WRITER_CASE release_and_withdrawal_retry_the_same_call_after_real_file_reopen passed"
    );
    Ok(())
}

async fn cases(
    base: &str,
    path: &Path,
    client: &ConformanceClient,
    http: &reqwest::Client,
    peer: &mut HolderConnection,
    database: &mut ReplicatedDatabase<HolderSchema>,
    tasks: &mut JoinSet<ConformanceResult<Value>>,
) -> ConformanceResult<()> {
    let mut write = database.begin_write().await?;
    (&mut write).execute("INSERT INTO holder_records (record_id, amount) VALUES ('019f0000-0000-7000-8000-000000000704', 41)").await?;
    write.commit().await?;
    let before_release = database.snapshot().await?.position();
    let release = ReleaseRequest {
        database: DatabaseId::try_from(DATABASE.to_owned())?,
        epoch: Epoch::new(1)?,
        transfer: TransferId::try_from("019f0000-0000-7000-8000-000000000705".to_owned())?,
    };
    start(
        http,
        base,
        tasks,
        json!({"attempt":"019f0000-0000-7000-8000-000000000706","holder":peer.peer,"request":{"kind":"release","value":release}}),
    );
    let (lost_frame, first) = received(peer.socket()?).await?;
    let ServerCall::Release {
        call: release_call,
        request,
    } = first
    else {
        return Err("caller did not send Release".into());
    };
    if request != release {
        return Err("caller changed release request".into());
    }
    let first_answer = database
        .request_release(&request, release_call.clone())
        .await?;
    if first_answer
        != (ReleaseAnswer::Released {
            final_position: before_release,
        })
    {
        return Err(format!(
            "writer did not freeze the committed capture position: {first_answer:?}"
        )
        .into());
    }
    peer.close().await?;
    pending_reply(tasks).await?;
    uncertain(http, base, release_call.operation).await?;
    database.close().await;
    *database = writer(path, peer.peer).await?;
    if !matches!(database.writer_state().await?, WriterState::Released { final_position, .. } if final_position == before_release)
        || !matches!(database.begin_write().await, Err(WriteRefusal::Released))
        || amount(database).await? != 41
    {
        return Err("reopened writer lost release authority or committed data".into());
    }
    peer.reconnect(client).await?;
    let (retry_frame, retry) = received(peer.socket()?).await?;
    let ServerCall::Release { call, request } = retry else {
        return Err("retry did not send Release".into());
    };
    if call != release_call || request != release || retry_frame == lost_frame {
        return Err("release retry changed its durable call/request or reused a frame".into());
    }
    let replay = database.request_release(&request, call.clone()).await?;
    if replay != first_answer
        || !matches!(database.begin_write().await, Err(WriteRefusal::Released))
    {
        return Err("release replay changed its frozen answer or reopened writes".into());
    }
    let mut conflict = request;
    conflict.transfer = TransferId::try_from("019f0000-0000-7000-8000-000000000709".to_owned())?;
    if database.request_release(&conflict, call).await? != ReleaseAnswer::RequestConflict {
        return Err("real writer did not retain the original call binding".into());
    }
    send(
        peer.socket()?,
        retry_frame,
        ServerCallAnswer::Release(replay.clone()),
    )
    .await?;
    completed(
        http,
        base,
        release_call.operation,
        "release",
        serde_json::to_value(replay)?,
    )
    .await?;

    let withdrawal = WithdrawRelease {
        database: release.database,
        epoch: release.epoch,
        transfer: release.transfer,
    };
    start(
        http,
        base,
        tasks,
        json!({"attempt":"019f0000-0000-7000-8000-000000000707","holder":peer.peer,"request":{"kind":"withdrawal","value":withdrawal}}),
    );
    let (lost_frame, first) = received(peer.socket()?).await?;
    let ServerCall::WithdrawRelease {
        call: withdrawal_call,
        request,
    } = first
    else {
        return Err("caller did not send withdrawal".into());
    };
    if request != withdrawal {
        return Err("caller changed withdrawal request".into());
    }
    if database
        .withdraw_release(&request, withdrawal_call.clone())
        .await?
        != WithdrawAnswer::Withdrawn
    {
        return Err("real holder did not withdraw its release".into());
    }
    // Lose the answer without closing the healthy connection: this time the
    // production object's existing alarm owner must expire the hub waiter.
    database.close().await;
    *database = writer(path, peer.peer).await?;
    let mut write = database.begin_write().await?;
    (&mut write).execute("UPDATE holder_records SET amount = 42 WHERE record_id = '019f0000-0000-7000-8000-000000000704'").await?;
    write.commit().await?;
    let after_withdrawal = database.snapshot().await?.position();
    pending_reply(tasks).await?;
    uncertain(http, base, withdrawal_call.operation).await?;
    let (retry_frame, retry) = received(peer.socket()?).await?;
    let ServerCall::WithdrawRelease { call, request } = retry else {
        return Err("retry did not send withdrawal".into());
    };
    if call != withdrawal_call || request != withdrawal || retry_frame == lost_frame {
        return Err("withdrawal retry changed its durable call/request or reused a frame".into());
    }
    let replay = database.withdraw_release(&request, call).await?;
    if replay != WithdrawAnswer::Withdrawn
        || amount(database).await? != 42
        || database.snapshot().await?.position() != after_withdrawal
    {
        return Err("withdrawal replay changed admitted data or capture position".into());
    }
    if database.request_release(&release, release_call).await?
        != ReleaseAnswer::ReleaseRefused(ReleaseRefusal::Withdrawn)
    {
        return Err("late release did not respect the durable withdrawal".into());
    }
    send(
        peer.socket()?,
        retry_frame,
        ServerCallAnswer::WithdrawRelease(replay),
    )
    .await?;
    completed(
        http,
        base,
        withdrawal_call.operation,
        "withdrawal",
        serde_json::to_value(replay)?,
    )
    .await?;
    Ok(())
}
