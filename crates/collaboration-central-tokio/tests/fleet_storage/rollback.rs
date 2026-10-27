//! Inject after actual writes, before the real Turso transaction commits.
use super::support::{Fixture, Io, TestResult, bundle, import_bytes};
use collaboration_central_core::{
    CoreHost, FleetKind, LocalSectionFailure, ObjectHost, ObjectRuntime, RegistrationEdits,
    RegistrationSnapshot, SocketFailure,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, ReborrowScope, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{CloseReason, ConnectionId};
use fleet_sync_session::Bytes;
use fleet_sync_tokio_runner::TokioObject;
use sqlx_core::{query::query, row::Row};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

type Native = TokioObject<CoreHost<Io>>;
struct FaultStore {
    object: Arc<Native>,
    armed: Arc<AtomicBool>,
    injected: Arc<AtomicBool>,
}
impl ReborrowScope for FaultStore {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        Native::reborrow(scope)
    }
}
impl AtomicStore for FaultStore {
    type Scope<'transaction> = <Native as AtomicStore>::Scope<'transaction>;
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
        let armed = self.armed.clone();
        let injected = self.injected.clone();
        self.object
            .transaction(move |scope| {
                Box::pin(async move {
                    let output = run(scope).await?;
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS count FROM project_directory",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    let count: i64 = row.try_get("count").map_err(StoreError::backend)?;
                    if count == 1 && armed.swap(false, Ordering::SeqCst) {
                        injected.store(true, Ordering::SeqCst);
                        return Err(StoreError::Aborted {
                            reason:
                                "injected after the actual directory, repository and feed writes"
                                    .into(),
                        });
                    }
                    Ok(output)
                })
            })
            .await
    }
}
struct FaultHost {
    native: Arc<TokioStorageHost<CoreHost<Io>>>,
    store: Arc<FaultStore>,
}
impl ObjectHost for FaultHost {
    type Database = sqlx_turso::Turso;
    type Store = FaultStore;
    type Wake = CoreHost<Io>;
    fn store(&self) -> &Arc<Self::Store> {
        &self.store
    }
    fn wake(&self) -> &Self::Wake {
        self.native.wake()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl Future<Output = Output> + Send + 'static,
    ) -> Result<Output, LocalSectionFailure> {
        self.native.with_local_section(run).await
    }
    async fn quarantine_connections(&self, to: Vec<ConnectionId>) {
        self.native.quarantine_connections(to).await;
    }
    async fn send_frame(&self, to: ConnectionId, frame: Bytes) -> Result<(), SocketFailure> {
        self.native.send_frame(to, frame).await
    }
    async fn close_connection(
        &self,
        to: ConnectionId,
        reason: CloseReason,
    ) -> Result<(), SocketFailure> {
        self.native.close_connection(to, reason).await
    }
    async fn registrations(&self) -> Result<RegistrationSnapshot, SocketFailure> {
        self.native.registrations().await
    }
    async fn admission_origin(
        &self,
        to: ConnectionId,
    ) -> Result<collaboration_central_core::AdmissionOrigin, SocketFailure> {
        self.native.admission_origin(to).await
    }
    async fn admission_binding(
        &self,
        to: ConnectionId,
    ) -> Result<fleet_sync_session::ConnectionBinding, SocketFailure> {
        self.native.admission_binding(to).await
    }
    async fn publish_registrations(
        &self,
        edits: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        self.native.publish_registrations(edits).await
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn failure_after_all_import_writes_rolls_back_every_canonical_table() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let injected = Arc::new(AtomicBool::new(false));
    let runtime = ObjectRuntime::<FleetKind, _, _>::fleet(
        Arc::new(FaultHost {
            native: Arc::new(TokioStorageHost::new(fixture.object.clone())),
            store: Arc::new(FaultStore {
                object: fixture.object.clone(),
                armed: Arc::new(AtomicBool::new(true)),
                injected: injected.clone(),
            }),
        }),
        Io::default(),
        fixture.receiver,
    )?;
    let proof = async {
        let offered = bundle()?;
        let call = import_bytes(&offered)?;
        assert!(matches!(
            runtime.on_call(call.clone()).await,
            Err(CallFailure::AnswerLost)
        ));
        assert!(injected.load(Ordering::SeqCst));
        assert_eq!(fixture.counts().await?, vec![0, 0, 0, 0, 0]);
        let answer: collaboration_central_core::FleetResult = serde_json::from_slice(
            &runtime
                .on_call(call)
                .await
                .map_err(|error| format!("retry failed: {error:?}"))?,
        )?;
        assert!(matches!(
            answer,
            collaboration_central_core::FleetResult::Imported(
                fleet_admission::FleetImportResult::Imported { .. }
            )
        ));
        assert_eq!(fixture.counts().await?, vec![1, 1, 1, 1, 3]);
        fixture.assert_import_headers().await
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn creation_directory_ledger_and_binding_roll_back_together_then_retry_once() -> TestResult {
    use super::metadata::request;
    use fleet_admission::{FleetOperation, FleetRequestResult};
    use message_board::{
        BoardId, BoardKind, Description, ProjectId, RequestDecision, ResourceName,
    };
    use serde_json::json;
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let board = BoardId::try_from(durable_steps::Uuid::now_v7().to_string())?;
    let project = ProjectId::try_from(durable_steps::Uuid::now_v7().to_string())?;
    let proof=async {
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("atomic creation workspace".to_owned())?,description:Description::try_from("real writes".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("atomic creation board".to_owned())?,description:Description::try_from("real parent".to_owned())?,board_kind:BoardKind::Area {}},
        ] {
            assert!(matches!(fixture.fleet_request(&request(operation)?).await?,FleetRequestResult::Decided(RequestDecision::Accepted {..})));
        }
        let creation=request(FleetOperation::ProjectDirectoryCreate {project,board,title:ResourceName::try_from("atomic creation".to_owned())?})?;
        let injected=Arc::new(AtomicBool::new(false));
        let runtime=ObjectRuntime::<FleetKind,_,_>::fleet(Arc::new(FaultHost {
            native:Arc::new(TokioStorageHost::new(fixture.object.clone())),
            store:Arc::new(FaultStore {object:fixture.object.clone(),armed:Arc::new(AtomicBool::new(true)),injected:injected.clone()}),
        }),Io::default(),fixture.receiver)?;
        let call=serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"atomic_creation"},
            "kind":"central.fleet-metadata","target":null,"request":{"request":creation,"origin":{"installation":"550e8400-e29b-41d4-a716-4466554400a9","machine":"live router"}}
        }))?;
        let before=fixture.counts().await?;
        assert!(matches!(runtime.on_call(call.clone()).await,Err(CallFailure::AnswerLost)));
        assert!(injected.load(Ordering::SeqCst),"fault did not reach the actual directory write");
        assert_eq!(fixture.counts().await?,before,"directory/feed escaped rollback");
        let request_id=creation.request_id.as_str().to_owned();
        let saved_request=request_id.clone();
        let counts=fixture.object.transaction(move |scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM creation_decisions) AS bindings,(SELECT count(*) FROM fleet_request_decisions WHERE request_id=?1) AS decisions")
                .bind(saved_request).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<i64,_>("bindings").map_err(StoreError::backend)?,row.try_get::<i64,_>("decisions").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(counts,(0,0),"creation index or its ledger escaped rollback");
        let bytes=runtime.on_call(call).await.map_err(|error|format!("same creation retry failed: {error:?}"))?;
        assert!(matches!(serde_json::from_slice::<FleetRequestResult>(&bytes)?,FleetRequestResult::Decided(RequestDecision::Accepted {..})));
        let counts=fixture.object.transaction(move |scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM creation_decisions) AS bindings,(SELECT count(*) FROM fleet_request_decisions WHERE request_id=?1) AS decisions")
                .bind(request_id).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<i64,_>("bindings").map_err(StoreError::backend)?,row.try_get::<i64,_>("decisions").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(counts,(1,1),"retry did not save one binding and one decision");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
