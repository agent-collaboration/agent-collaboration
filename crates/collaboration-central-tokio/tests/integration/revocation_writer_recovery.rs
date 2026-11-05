#![cfg(feature = "conformance")]
//! A real producer keeps its call pending across terminal revoke and a fresh session.
#[path = "../support/revocation_writer_transport.rs"]
mod transport;
use crate::{configuration, creation_scenarios};
use board_fleet_sync::{BoardProjectSchema, MachineSessionStore};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
};
use collaboration_central_core::{
    RevocationControl, RevocationControlInput, RevocationObservation,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::{
    AtomicStore, CallFailure, CallId, Engine, OperationState, StoreError,
    storage::{JournalStorage, StepState},
};
use fleet_sync_protocol::{PeerId, ReceiverId, SchemaSupport};
use fleet_sync_session::{ClientEvent, Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::{
    CallEnvelope, CallRouter, SessionPump, TokioHost, TokioRunner, WriterSessionCalls,
};
use fleet_sync_writer::{
    RegisterDatabase, RenewLease, ReplicatedDatabase, SeedCopy, WriterOptions,
    WriterRegistrationOutcome,
};
use runtime_primitives::{Backoff, Clock, SyncTime, UuidV7Ids};
use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400e9";
const PEER: &str = "019f0000-0000-7000-8000-000000001008";
const CREDENTIAL: &str = "revoked-writer-recovery";
const BOUND: Duration = Duration::from_secs(15);
#[derive(Clone)]
struct WriterClock(Arc<AtomicI64>);
impl Clock for WriterClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
struct SeedHold {
    armed: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
struct ObservedCalls {
    hold: Option<Arc<SeedHold>>,
    calls: WriterSessionCalls<BoardProjectSchema, BoardConsumerProtocol>,
    observed: mpsc::UnboundedSender<(CallId, bool, serde_json::Value)>,
}
impl CallRouter for ObservedCalls {
    async fn dispatch(&self, call: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        if let Some(hold) = &self.hold
            && call.kind == "fleet.writer-seed-file"
            && matches!(
                serde_json::from_value::<fleet_sync_writer::SeedFileRequest>(call.request.clone()),
                Ok(fleet_sync_writer::SeedFileRequest::Open { .. })
            )
            && hold.armed.swap(false, Ordering::SeqCst)
        {
            hold.entered.notify_one();
            hold.release.notified().await;
        }
        let identity = call.call.clone();
        let request = call.request.clone();
        let result = self.calls.dispatch(call).await;
        let retry = matches!(&result,Err(CallFailure::Retryable {reason}) if reason.code=="callerNotEnrolled");
        let _ = self.observed.send((identity, retry, request));
        result
    }
}
type WriterHost = TokioHost<ObservedCalls, WriterClock>;
type WriterStore = fleet_sync_writer::WriterJournalStore<BoardProjectSchema, WriterHost>;
async fn control(
    base: &str,
    command: RevocationControl,
) -> ConformanceResult<RevocationObservation> {
    Ok(reqwest::Client::new()
        .post(format!("{base}/admin/v1/conformance/revocation"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&RevocationControlInput {
            service: SERVICE.to_owned().try_into()?,
            control: command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn revoked(base: &str, completed: bool) -> ConformanceResult<()> {
    tokio::time::timeout(BOUND, async {
        loop {
            let row = control(base, RevocationControl::Observe).await?;
            if row.enrollment.is_some_and(|row| row.revoked)
                && if completed {
                    matches!(
                        row.operation.map(|op| op.state),
                        Some(OperationState::Completed { .. })
                    )
                } else {
                    row.gate_reached
                }
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok(())
}
async fn owner(
    database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
    receiver: ReceiverId,
    requests: fleet_sync_tokio_runner::SessionRequests<BoardConsumerProtocol>,
    clock: WriterClock,
    observed: mpsc::UnboundedSender<(CallId, bool, serde_json::Value)>,
    hold: Option<Arc<SeedHold>>,
) -> ConformanceResult<(
    Arc<WriterStore>,
    Arc<Engine<WriterStore, WriterHost>>,
    Arc<WriterHost>,
)> {
    let host = Arc::new(TokioHost::new(
        Arc::new(ObservedCalls {
            hold,
            calls: WriterSessionCalls::new(database.clone(), receiver, requests),
            observed,
        }),
        clock,
    ));
    let store = Arc::new(database.journal_store(host.clone()).await?);
    let mut engine = Engine::new(store.clone(), host.clone());
    engine.register(RegisterDatabase::new(host.clone()))?;
    engine.register(SeedCopy::new(host.clone()))?;
    engine.register(RenewLease::new())?;
    Ok((store, Arc::new(engine), host))
}
async fn pending(store: &Arc<WriterStore>, expected: &CallId) -> ConformanceResult<CallId> {
    let expected = expected.clone();
    tokio::time::timeout(BOUND,async {loop {
        let expected=expected.clone();
        let found=store.transaction(move |scope|Box::pin(async move {
            let operation=scope.operation(expected.operation).await?.ok_or_else(||StoreError::backend("operation disappeared"))?;
            if !matches!(operation.status().state,OperationState::Running {..}) {return Err(StoreError::backend("producer terminalized its enrollment refusal"));}
            for position in 0..scope.step_count(operation.id).await? {
                if let Some(row)=scope.step(operation.id,position).await? && row.name==expected.step {
                    return Ok(matches!(&row.state,StepState::CallPending {attempt,..} if attempt.retry_reason.as_ref().is_some_and(|reason|reason.code=="callerNotEnrolled")).then_some(expected));
                }
            }
            Ok(None)
        })).await?;
        if let Some(call)=found {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(call);}
        tokio::task::yield_now().await;
    }}).await?
}
#[derive(Clone, Copy, Debug)]
enum RecoveryCase {
    Registration,
    Renewal,
    SeedOpen,
}
async fn run_recovery(case: RecoveryCase) -> ConformanceResult<()> {
    let renewal = matches!(case, RecoveryCase::Renewal);
    let seeded = matches!(case, RecoveryCase::SeedOpen);
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().join("central"),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let base = server.base_url();
    let client = ConformanceClient::new(base)?;
    let peer: PeerId = PEER.to_owned().try_into()?;
    let database = Arc::new(if seeded {
        transport::adopted_writer(&root.path().join("writer.db"), peer).await?
    } else {
        ReplicatedDatabase::create(
            &root.path().join("writer.db"),
            BoardProjectSchema::new()?,
            peer,
            fleet_sync_lease::LeasePolicy {
                lease_duration: Duration::from_secs(14400),
                renew_interval: Duration::from_secs(900),
            },
            WriterOptions::default(),
        )
        .await?
    });
    let mut setup = collaboration_central_conformance::enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        CREDENTIAL,
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let workspace = durable_steps::Uuid::now_v7().to_string().try_into()?;
    let board: message_board::BoardId = durable_steps::Uuid::now_v7().to_string().try_into()?;
    for operation in [
        fleet_admission::FleetOperation::WorkspaceCreate {
            name: "writer recovery".to_owned().try_into()?,
            description: "revocation proof".to_owned().try_into()?,
        },
        fleet_admission::FleetOperation::BoardCreate {
            board: board.clone(),
            name: "writer recovery".to_owned().try_into()?,
            description: "revocation proof".to_owned().try_into()?,
            board_kind: message_board::BoardKind::Area {},
        },
        creation_scenarios::project(database.database(), board)?,
    ] {
        creation_scenarios::accepted(
            creation_scenarios::metadata(&mut setup.socket, &workspace, operation).await?,
        )?;
    }
    setup
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    let transport = transport::FixtureTransport::default();
    let sends = transport.sends.clone();
    let state_dir = root.path().join("machine");
    let session = SessionClient::<_, _, BoardConsumerProtocol>::load(
        transport,
        MachineSessionStore::open(&state_dir).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 7 },
        Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_millis(100),
            factor: 2,
        },
    )
    .await?;
    let old_session = session.state().session_id;
    let (requests, mut events, pump) = SessionPump::new(
        session,
        Endpoint::new(&format!("{base}/v1/session"))?,
        PeerCredential::new(CREDENTIAL)?,
        BOUND,
    );
    let (stop, stopped) = watch::channel(false);
    let pump_task = tokio::spawn(pump.run(stopped.clone()));
    tokio::time::timeout(BOUND, async {
        loop {
            if matches!(events.recv().await, Some(ClientEvent::Welcome { .. })) {
                break;
            }
        }
    })
    .await?;
    let clock = WriterClock(Arc::new(AtomicI64::new(1792281600000)));
    let receiver = ReceiverId::new(durable_steps::Uuid::now_v7())?;
    let (observed, mut observed_rx) = mpsc::unbounded_channel();
    let seed_hold = seeded.then(|| {
        Arc::new(SeedHold {
            armed: std::sync::atomic::AtomicBool::new(true),
            ..Default::default()
        })
    });
    let (store, engine, host) = owner(
        database.clone(),
        receiver,
        requests.clone(),
        clock.clone(),
        observed.clone(),
        seed_hold.clone(),
    )
    .await?;
    let runner = TokioRunner::new(engine.clone(), host.clone());
    let runner_task = tokio::spawn(async move { runner.run(stopped).await });
    let mut initial_registration = None;
    if renewal || seeded {
        initial_registration = Some(store.start_registration(receiver).await?);
        let (_, retry, _) = tokio::time::timeout(BOUND, observed_rx.recv())
            .await?
            .ok_or("initial registration observation ended")?;
        if retry {
            return Err("enrolled registration control was refused".into());
        }
        if let Some(hold) = &seed_hold {
            tokio::time::timeout(BOUND, hold.entered.notified())
                .await
                .map_err(|_| "seed-open producer did not reach its pre-dispatch barrier")?;
        } else {
            let mut changed = database.subscribe_authority_changes();
            tokio::time::timeout(BOUND, async {
                loop {
                    if matches!(
                        database.registration_outcome().await?,
                        WriterRegistrationOutcome::Ready { .. }
                    ) {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    changed.changed().await?;
                }
            })
            .await??;
        }
    }
    control(base, RevocationControl::ArmDelivery).await?;
    let revoke_base = base.to_owned();
    let revoke = tokio::spawn(async move {
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            reqwest::Client::new()
                .post(format!("{revoke_base}/admin/v1/machines/revoke"))
                .header(
                    "Cf-Access-Jwt-Assertion",
                    collaboration_central_conformance::owner_assertion()?,
                )
                .json(&serde_json::json!({"kind":"revokeMachine","value":{"installation":SERVICE}}))
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await?,
        )
    });
    revoked(base, false).await?;
    let operation = if renewal {
        store.prompt_renewal(receiver).await?
    } else if seeded {
        initial_registration.ok_or("seed registration was not started")?
    } else {
        store.start_registration(receiver).await?
    };
    if let Some(hold) = &seed_hold {
        hold.release.notify_one();
    }
    let (first, retry, first_request) = tokio::time::timeout(BOUND, observed_rx.recv())
        .await?
        .ok_or("adapter observation ended")?;
    if !retry || first.operation != operation {
        return Err("real registration did not classify callerNotEnrolled as retryable".into());
    }
    let pending_call = pending(&store, &first).await?;
    if first != pending_call {
        return Err("pending call identity changed".into());
    }
    control(base, RevocationControl::ReleaseDelivery).await?;
    tokio::time::timeout(BOUND, revoke).await???;
    let terminal = tokio::time::timeout(BOUND, pump_task).await??;
    if !matches!(terminal, Err(fleet_sync_session::ClientError::SessionEnded)) {
        return Err(format!("pump did not terminate on revoke: {terminal:?}").into());
    }
    stop.send_replace(true);
    tokio::time::timeout(BOUND, runner_task).await???;
    if !requests.is_closed() {
        return Err("old pump command channel stayed open".into());
    }
    let stopped_sends = sends.load(Ordering::SeqCst);
    clock.0.fetch_add(120000, Ordering::SeqCst);
    pending(&store, &first).await?;
    if sends.load(Ordering::SeqCst) != stopped_sends {
        return Err("stopped session sent another frame".into());
    }
    drop(store);
    drop(engine);
    drop(host);
    revoked(base, true).await?;
    transport::enroll(base, SERVICE, peer, CREDENTIAL).await?;
    tokio::time::timeout(BOUND, async {
        loop {
            let row = control(base, RevocationControl::Observe).await?;
            if row.enrollment.is_some_and(|row| !row.revoked)
                && matches!(
                    row.readmission_operation.map(|op| op.state),
                    Some(OperationState::Completed { .. })
                )
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let mut session = SessionClient::<_, _, BoardConsumerProtocol>::load(
        transport::FixtureTransport::default(),
        MachineSessionStore::open(&state_dir).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 7 },
        Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_millis(100),
            factor: 2,
        },
    )
    .await?;
    session.start_new_session(&UuidV7Ids).await?;
    let new_session = session.state().session_id;
    if new_session == old_session {
        return Err("readmission reused the ended session".into());
    }
    let (requests, mut events, pump) = SessionPump::new(
        session,
        Endpoint::new(&format!("{base}/v1/session"))?,
        PeerCredential::new(CREDENTIAL)?,
        BOUND,
    );
    let (stop, stopped) = watch::channel(false);
    let pump_task = tokio::spawn(pump.run(stopped.clone()));
    tokio::time::timeout(BOUND, async {
        loop {
            if matches!(events.recv().await, Some(ClientEvent::Welcome { .. })) {
                break;
            }
        }
    })
    .await?;
    let (_store, engine, host) =
        owner(database.clone(), receiver, requests, clock, observed, None).await?;
    let runner = TokioRunner::new(engine.clone(), host);
    let runner_task = tokio::spawn(async move { runner.run(stopped).await });
    let (recovered, retry, recovered_request) = tokio::time::timeout(BOUND, observed_rx.recv())
        .await?
        .ok_or("recovery observation ended")?;
    if retry || recovered != first || recovered_request != first_request {
        return Err("fresh session did not recover the same registration CallId".into());
    }
    let mut changed = database.subscribe_authority_changes();
    tokio::time::timeout(BOUND, async {
        loop {
            if matches!(
                database.registration_outcome().await?,
                WriterRegistrationOutcome::Ready { .. }
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            changed.changed().await?;
        }
    })
    .await??;
    tokio::time::timeout(BOUND, async {
        loop {
            let finished = _store
                .transaction(move |scope| {
                    Box::pin(async move {
                        match scope.operation(operation).await? {
                            None => Ok(true),
                            Some(row) => match row.status().state {
                                OperationState::Completed { .. } => Ok(true),
                                OperationState::Running { .. } => Ok(false),
                                OperationState::Failed { .. } => {
                                    Err(StoreError::backend("recovered producer failed"))
                                }
                            },
                        }
                    })
                })
                .await?;
            if finished {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    println!(
        "WRITER_ENROLLMENT_RECOVERY {case:?} old_session={old_session:?} new_session={new_session:?} call={first:?} request={first_request} completed_once"
    );
    stop.send_replace(true);
    tokio::time::timeout(BOUND, pump_task).await???;
    tokio::time::timeout(BOUND, runner_task).await???;
    drop(_store);
    database.close().await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn registration_recovers_the_same_journal_call_after_terminal_revoke_and_new_session()
-> ConformanceResult<()> {
    run_recovery(RecoveryCase::Registration).await
}
#[tokio::test]
async fn renewal_recovers_its_memoized_request_and_call_after_terminal_revoke_and_new_session()
-> ConformanceResult<()> {
    run_recovery(RecoveryCase::Renewal).await
}

#[tokio::test]
async fn seeded_registration_recovers_the_same_seed_generation_after_terminal_revoke_and_new_session()
-> ConformanceResult<()> {
    run_recovery(RecoveryCase::SeedOpen).await
}
