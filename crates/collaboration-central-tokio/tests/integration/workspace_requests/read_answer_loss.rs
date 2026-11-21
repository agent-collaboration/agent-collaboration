use super::*;
use central_link_protocol::{ReplyPayload, RequestScope, RequestStatusAnswer, RequestStatusQuery};
use central_storage::{StoreCall, StoreFailure, StoreReply};
use collaboration_central_core::WorkspaceRuntime;
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceStores};
use fleet_contracts::{
    ReadWorkspaceRequestStatus, WorkspaceRequest, WorkspaceResponse, WorkspaceServiceClient,
    WorkspaceServiceProtocol, WorkspaceServiceRpc,
};
use fleet_rpc::{CallFailure, Envelope, Transport};
use fleet_runtime::{ClockIds, FleetRuntime};
use fleet_runtime_ports::{Clock, SyncTime, WorkspaceStores};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Clone)]
struct ArcClock(Arc<dyn router_clock::RuntimeClock>);
impl Clock for ArcClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(runtime_primitives::Clock::now(self.0.as_ref()).unix_millis())
    }
}
#[derive(Clone)]
struct LoseFirstRead {
    stores: TokioWorkspaceStores,
    lose: Arc<AtomicBool>,
}
impl WorkspaceStores for LoseFirstRead {
    async fn call_workspace(
        &self,
        workspace: &m::WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        let is_read = matches!(call, StoreCall::Read(_));
        let reply = self.stores.call_workspace(workspace, call).await?;
        if is_read && self.lose.swap(false, Ordering::SeqCst) {
            return Err(StoreFailure::AnswerLost);
        }
        Ok(reply)
    }
}

impl Transport<WorkspaceServiceProtocol> for LoseFirstRead {
    async fn exchange(
        &self,
        request: Envelope<m::WorkspaceId, WorkspaceRequest>,
    ) -> Result<Envelope<m::WorkspaceId, WorkspaceResponse>, CallFailure> {
        let reply = self.stores.exchange(request).await?;
        if self.lose.swap(false, Ordering::SeqCst) {
            return Err(CallFailure::AnswerLost);
        }
        Ok(reply)
    }
}
#[tokio::test]
async fn lost_read_answer_before_effect_is_unavailable_and_never_cached() {
    let f = Fixture::new().await;
    f.area(890).await;
    f.host.close().await;
    let due_batches = Arc::new(fleet_runtime::DueBatchState::default());
    let clock = ArcClock(f.shared_for_runtime());
    let central = Arc::new(
        ObjectStorage::new(
            f.root
                .path()
                .join("central")
                .join(format!("{}.sqlite", f.caller.organization_id().as_str())),
            Arc::new(fleet_storage_schema::workspace_migrations::central_migrator()),
        )
        .with_engine_publication(
            Arc::new(clock.clone()),
            fleet_runtime::engine_alarm_decider(due_batches.clone()),
        ),
    );
    let store = Arc::new(ObjectStorage::new(
        f.root
            .path()
            .join("workspace-stores")
            .join(format!("{}.sqlite", f.workspace)),
        Arc::new(fleet_storage_schema::workspace_migrations::workspace_migrator()),
    ));
    let lose = Arc::new(AtomicBool::new(true));
    let runtime = WorkspaceRuntime::new(FleetRuntime::open(
        f.caller.organization_id().clone(),
        central.clone(),
        LoseFirstRead {
            stores: TokioWorkspaceStores::with_fixture_store(
                f.root.path().join("workspace-stores"),
                f.workspace.clone(),
                store.clone(),
            ),
            lose: lose.clone(),
        },
        Arc::new(clock.clone()),
        Arc::new(ClockIds::new(clock.clone())),
        due_batches,
        Arc::new(|_| {}),
    ))
    .with_sign_in_verification(
        collaboration_central_tokio::NativeSignInTrust::new(settings(&f.keys.url))
            .unwrap()
            .for_organization(f.caller.organization_id().clone(), Arc::new(clock))
            .unwrap(),
    );
    let request = m::WorkspaceRequest {
        request_id: Some(id(891)),
        workspace_id: Some(f.workspace.clone()),
        operation: m::WorkspaceOperation::CreateArea {
            name: name("After read loss"),
            description: "Test description".to_owned().try_into().unwrap(),
        },
    };
    assert_eq!(
        runtime
            .request(&f.caller, request.clone())
            .await
            .unwrap()
            .result,
        m::WorkspaceResult::CentralUnavailable
    );
    assert!(
        store
            .read(Statement {
                sql: "SELECT request_id FROM workspace_request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(891))]
            })
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    let replay = runtime.request(&f.caller, request.clone()).await.unwrap();
    assert!(matches!(
        replay.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    assert_eq!(runtime.request(&f.caller, request).await.unwrap(), replay);

    let status = RequestStatusQuery {
        request_id: id(891),
        scope: RequestScope::Workspace(f.workspace.clone()),
    };
    lose.store(true, Ordering::SeqCst);
    assert_eq!(
        runtime.request_status(&f.caller, &status).await,
        Err(StoreFailure::AnswerLost)
    );
    assert_eq!(
        runtime.request_status(&f.caller, &status).await.unwrap(),
        RequestStatusAnswer::Answered(ReplyPayload::Workspace(replay))
    );
    central.close().await;
    store.close().await;
}

#[tokio::test]
async fn request_status_returns_a_typed_reply_when_the_workspace_store_cannot_open() {
    let root = tempfile::tempdir().unwrap();
    let workspace = id::<m::WorkspaceId>(895);
    let store = Arc::new(ObjectStorage::new(
        root.path().join("workspace.sqlite"),
        Arc::new(fleet_storage_schema::workspace_migrations::failing_upgrade(
            fleet_storage_schema::workspace_migrations::workspace_migrator(),
        )),
    ));
    let stores =
        TokioWorkspaceStores::with_fixture_store(root.path().into(), workspace.clone(), store);
    let client = WorkspaceServiceClient::new(
        workspace,
        LoseFirstRead {
            stores,
            lose: Arc::new(AtomicBool::new(true)),
        },
    );
    let status_call = || ReadWorkspaceRequestStatus {
        request_id: id(896),
        actor: id(897),
        cutoff: "2026-11-20T12:00:00Z".parse().unwrap(),
    };

    // The fixture can lose the first reply only after the real transport returns an envelope.
    assert_eq!(
        client.read_request_status(status_call()).await,
        Err(StoreFailure::AnswerLost)
    );
    assert_eq!(
        client.read_request_status(status_call()).await,
        Err(StoreFailure::Unavailable)
    );
}
