#![cfg(feature = "conformance")]

use collaboration_central_core::{FleetDecisionFault, ObjectHost, StepConformance};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::storage::TransactionScope;
use durable_steps::{AtomicStore, BoxFuture, CallFailure, CallId, CallTarget, Host, StoreError};
use fleet_admission::FleetRequestResult;
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore, TokioObject};
use message_board::*;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, oneshot};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;
type ReceiptDriver = StepConformance<TokioStorageHost<FixtureHost>>;

#[path = "step_transactions/native_unwind.rs"]
mod native_unwind;
#[path = "step_transactions/validation.rs"]
mod validation;

struct FixtureHost;

impl Clock for FixtureHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for FixtureHost {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for FixtureHost {
    async fn wake_at(&self, _at: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for FixtureHost {
    async fn dispatch<Call: CallTarget>(
        &self,
        _target: Call,
        _request: Call::Request,
        _call: CallId,
    ) -> Result<Call::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: durable_steps::NotSentReason::Overloaded,
        })
    }
}

struct Fixture {
    objects: ObjectStore<FixtureHost>,
    object: Arc<TokioObject<FixtureHost>>,
    driver: ReceiptDriver,
}

impl Fixture {
    async fn open(directory: &Path, initialize: bool) -> Result<Self, TestError> {
        let objects = ObjectStore::open(directory).await?;
        let object = objects
            .object(
                ObjectKey::new("fleet")?,
                ObjectOptions::default(),
                Arc::new(FixtureHost),
            )
            .await?;
        let driver = StepConformance::new(
            Arc::new(TokioStorageHost::new(object.clone())),
            Arc::new(FixtureHost),
            runtime_primitives::Clock::now(&FixtureHost),
        );
        if initialize {
            driver.initialize().await?;
        }
        Ok(Self {
            objects,
            object,
            driver,
        })
    }
}

fn receipt() -> Result<RequestLedgerEntry, TestError> {
    collaboration_central_conformance::receipt_fixture(RequestId::try_from(
        "019f0000-0000-7000-8000-000000000001".to_owned(),
    )?)
}

fn accepted_decision() -> Result<RequestDecision, TestError> {
    Ok(RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_page(
            Vec::new(),
            &DecisionEffectsHeader {
                count: 0,
                digest: CanonicalHash::try_from(
                    "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945".to_owned(),
                )?,
            },
            None,
        )?,
        change: ChangeOutcome::Changed,
    })
}

async fn stored_receipt_count(fixture: &Fixture) -> Result<String, TestError> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT CAST(COUNT(*) AS TEXT) AS count FROM fleet_decision_receipts",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("count")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn nonempty_receipts_survive_reopen_and_replay_the_original_effects() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let (entry, effects) = collaboration_central_conformance::receipt_with_effects_fixture(
        RequestId::try_from("019f0000-0000-7000-8000-0000000000a1".to_owned())?,
    )?;
    let answer = fixture
        .driver
        .record_fleet_decision(entry.clone(), effects.clone())
        .await?;
    let [FleetRequestResult::Decided(RequestDecision::Accepted { effects: page, .. })] =
        answer.as_slice()
    else {
        return Err("first nonempty receipt did not answer Accepted".into());
    };
    assert_eq!(page.entries(), effects.as_slice());
    assert_eq!(page.total_count(), 2);
    assert_eq!(stored_receipt_count(&fixture).await?, "2");
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path(), false).await?;
    let mut changed = entry.clone();
    changed.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    assert_eq!(
        fixture
            .driver
            .record_fleet_decision(changed, Vec::new())
            .await?,
        answer
    );
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry)
    );
    assert_eq!(stored_receipt_count(&fixture).await?, "2");
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn both_rollback_causes_discard_the_header_and_every_effect_row() -> TestResult {
    for fault in [
        FleetDecisionFault::CallbackAfterWrite,
        FleetDecisionFault::SqlAfterWrite,
    ] {
        let root = tempfile::tempdir()?;
        let fixture = Fixture::open(root.path(), true).await?;
        let (entry, effects) = collaboration_central_conformance::receipt_with_effects_fixture(
            RequestId::try_from("019f0000-0000-7000-8000-0000000000a2".to_owned())?,
        )?;
        let result = fixture
            .driver
            .record_with_fault(entry.clone(), effects.clone(), fault)
            .await;
        match fault {
            FleetDecisionFault::CallbackAfterWrite => {
                assert!(matches!(result, Err(StoreError::Aborted { .. })))
            }
            FleetDecisionFault::SqlAfterWrite => {
                assert!(matches!(result, Err(StoreError::Backend { .. })))
            }
            FleetDecisionFault::None => {
                return Err("rollback case needs a real injected failure".into());
            }
        }
        assert_eq!(
            fixture
                .driver
                .read_fleet_decision(entry.request_id.clone())
                .await?,
            None
        );
        assert_eq!(stored_receipt_count(&fixture).await?, "0");
        assert!(matches!(
            fixture
                .driver
                .record_fleet_decision(entry, effects)
                .await?
                .as_slice(),
            [FleetRequestResult::Decided(
                RequestDecision::Accepted { .. }
            )]
        ));
        assert_eq!(stored_receipt_count(&fixture).await?, "2");
        fixture.objects.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn committed_receipt_and_first_decision_survive_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path(), true).await?;
    let entry = receipt()?;

    let effects = fixture
        .driver
        .record_fleet_decision(entry.clone(), Vec::new())
        .await?;

    assert_eq!(
        effects,
        vec![FleetRequestResult::Decided(accepted_decision()?),]
    );
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry.clone())
    );
    fixture.objects.close().await?;
    drop(fixture);

    let fixture = Fixture::open(directory.path(), false).await?;
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry.clone())
    );
    let mut replay = entry.clone();
    replay.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    let effects = fixture
        .driver
        .record_fleet_decision(replay.clone(), Vec::new())
        .await?;
    assert_eq!(
        effects,
        vec![FleetRequestResult::Decided(accepted_decision()?),]
    );
    replay.canonical_hash = CanonicalHash::try_from("b".repeat(64))?;
    let effects = fixture
        .driver
        .record_fleet_decision(replay, Vec::new())
        .await?;
    assert_eq!(effects, vec![FleetRequestResult::ConflictingRequest]);
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry)
    );
    fixture.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn callback_error_discards_the_written_receipt_and_effects() -> TestResult {
    rollback_case(FleetDecisionFault::CallbackAfterWrite).await
}

#[tokio::test]
async fn sql_error_discards_the_written_receipt_and_effects() -> TestResult {
    rollback_case(FleetDecisionFault::SqlAfterWrite).await
}

async fn rollback_case(fault: FleetDecisionFault) -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path(), true).await?;
    let entry = receipt()?;

    let result = fixture
        .driver
        .record_with_fault(entry.clone(), Vec::new(), fault)
        .await;

    match fault {
        FleetDecisionFault::CallbackAfterWrite => {
            assert!(matches!(result, Err(StoreError::Aborted { .. })))
        }
        FleetDecisionFault::SqlAfterWrite => {
            assert!(matches!(result, Err(StoreError::Backend { .. })))
        }
        FleetDecisionFault::None => return Err("rollback scenario requires a failure".into()),
    }
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        None
    );
    let effects = fixture
        .driver
        .record_fleet_decision(entry, Vec::new())
        .await?;
    assert_eq!(
        effects,
        vec![FleetRequestResult::Decided(accepted_decision()?),]
    );
    fixture.objects.close().await?;
    Ok(())
}

struct HeldStore {
    inner: Arc<TokioObject<FixtureHost>>,
    callback_finished: Arc<Semaphore>,
    allow_commit: Arc<Semaphore>,
}

impl AtomicStore for HeldStore {
    type Scope<'transaction> = <TokioObject<FixtureHost> as AtomicStore>::Scope<'transaction>;

    async fn transaction<Output, Callback>(&self, run: Callback) -> Result<Output, StoreError>
    where
        Output: Send + 'static,
        Callback: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<Output, StoreError>>
            + Send
            + 'static,
    {
        let callback_finished = self.callback_finished.clone();
        let allow_commit = self.allow_commit.clone();
        self.inner
            .transaction(move |scope| {
                Box::pin(async move {
                    let output = run(scope).await?;
                    callback_finished.add_permits(1);
                    allow_commit
                        .acquire_owned()
                        .await
                        .map_err(StoreError::backend)?
                        .forget();
                    Ok(output)
                })
            })
            .await
    }
}

struct HeldHost {
    store: Arc<HeldStore>,
    native: TokioStorageHost<FixtureHost>,
}
impl ObjectHost for HeldHost {
    type Database = sqlx_turso::Turso;
    type Store = HeldStore;
    type Wake = FixtureHost;
    fn wake(&self) -> &Self::Wake {
        self.native.wake()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl std::future::Future<Output = Output> + Send + 'static,
    ) -> Result<Output, collaboration_central_core::LocalSectionFailure> {
        self.native.with_local_section(run).await
    }
    async fn quarantine_connections(&self, connections: Vec<fleet_sync_protocol::ConnectionId>) {
        self.native.quarantine_connections(connections).await;
    }
    async fn send_frame(
        &self,
        to: fleet_sync_protocol::ConnectionId,
        frame: fleet_sync_session::Bytes,
    ) -> Result<(), collaboration_central_core::SocketFailure> {
        self.native.send_frame(to, frame).await
    }
    async fn close_connection(
        &self,
        to: fleet_sync_protocol::ConnectionId,
        reason: fleet_sync_protocol::CloseReason,
    ) -> Result<(), collaboration_central_core::SocketFailure> {
        self.native.close_connection(to, reason).await
    }
    async fn admission_binding(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<fleet_sync_session::ConnectionBinding, collaboration_central_core::SocketFailure>
    {
        self.native.admission_binding(connection).await
    }
    async fn admission_origin(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<
        collaboration_central_core::AdmissionOrigin,
        collaboration_central_core::SocketFailure,
    > {
        self.native.admission_origin(connection).await
    }
    async fn registrations(
        &self,
    ) -> Result<
        collaboration_central_core::RegistrationSnapshot,
        collaboration_central_core::SocketFailure,
    > {
        self.native.registrations().await
    }
    async fn publish_registrations(
        &self,
        edits: collaboration_central_core::RegistrationEdits,
    ) -> Vec<(
        fleet_sync_protocol::ConnectionId,
        collaboration_central_core::SocketFailure,
    )> {
        self.native.publish_registrations(edits).await
    }

    fn store(&self) -> &Arc<Self::Store> {
        &self.store
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn effects_are_unavailable_while_the_real_transaction_waits_to_commit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path(), true).await?;
    let callback_finished = Arc::new(Semaphore::new(0));
    let allow_commit = Arc::new(Semaphore::new(0));
    let driver = StepConformance::new(
        Arc::new(HeldHost {
            store: Arc::new(HeldStore {
                inner: fixture.object.clone(),
                callback_finished: callback_finished.clone(),
                allow_commit: allow_commit.clone(),
            }),
            native: TokioStorageHost::new(fixture.object.clone()),
        }),
        Arc::new(FixtureHost),
        runtime_primitives::Clock::now(&FixtureHost),
    );
    let entry = receipt()?;
    let request_id = entry.request_id.clone();
    let (effect_sender, mut effect_receiver) = oneshot::channel();
    let step = tokio::spawn(async move {
        let effects = driver.record_fleet_decision(entry, Vec::new()).await?;
        effect_sender
            .send(effects)
            .map_err(|_| StoreError::Aborted {
                reason: "effect observer disappeared".into(),
            })
    });

    tokio::time::timeout(Duration::from_secs(5), callback_finished.acquire_owned())
        .await??
        .forget();
    assert!(matches!(
        effect_receiver.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    allow_commit.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), step).await???;

    assert_eq!(
        effect_receiver.await?,
        vec![FleetRequestResult::Decided(accepted_decision()?)]
    );
    assert!(
        fixture
            .driver
            .read_fleet_decision(request_id)
            .await?
            .is_some()
    );
    fixture.objects.close().await?;
    Ok(())
}
