use super::*;
use central_link_protocol::{ReplyPayload, RequestScope, RequestStatusAnswer, RequestStatusQuery};
use central_storage::{StoreCall, StoreFailure, StoreReply};
use collaboration_central_core::{ClockIds, WorkspaceRuntime};
use collaboration_central_tokio::ObjectStorage;
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
    store: Arc<ObjectStorage>,
    lose: Arc<AtomicBool>,
}
impl WorkspaceStores for LoseFirstRead {
    async fn call_workspace(
        &self,
        _workspace: &m::WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        match call {
            StoreCall::Read(statement) => {
                let answer = self.store.read(statement).await?;
                if self.lose.swap(false, Ordering::SeqCst) {
                    Err(StoreFailure::AnswerLost)
                } else {
                    Ok(StoreReply::Read(answer))
                }
            }
            StoreCall::Batch(batch) => self.store.run_batch(batch).await.map(StoreReply::Batch),
        }
    }
}
#[tokio::test]
async fn lost_read_answer_before_effect_is_unavailable_and_never_cached() {
    let f = Fixture::new().await;
    f.area(890).await;
    f.host.close().await;
    let due_batches = Arc::new(collaboration_central_core::DueBatchState::default());
    let clock = ArcClock(f.shared_for_runtime());
    let central = Arc::new(
        ObjectStorage::new(
            f.root
                .path()
                .join("central")
                .join(format!("{}.sqlite", f.caller.organization_id().as_str())),
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        )
        .with_engine_publication(
            Arc::new(clock.clone()),
            collaboration_central_core::engine_alarm_decider(due_batches.clone()),
        ),
    );
    let store = Arc::new(ObjectStorage::new(
        f.root
            .path()
            .join("workspace-stores")
            .join(format!("{}.sqlite", f.workspace)),
        Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    ));
    let lose = Arc::new(AtomicBool::new(true));
    let runtime = WorkspaceRuntime::new(
        f.caller.organization_id().clone(),
        central.clone(),
        LoseFirstRead {
            store: store.clone(),
            lose: lose.clone(),
        },
        Arc::new(clock.clone()),
        Arc::new(ClockIds::new(clock.clone())),
        due_batches,
    )
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
