use super::*;
use central_storage::{StoreCall, StoreFailure, StoreReply};
use collaboration_central_core::{ClockIds, WorkspaceRuntime, WorkspaceStoreCalls};
use collaboration_central_tokio::ObjectStorage;
use runtime_primitives::{Clock, SyncTime};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Clone)]
struct ArcClock(Arc<HeldClock>);
impl Clock for ArcClock {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}
#[derive(Clone)]
struct LoseFirstRead {
    store: Arc<ObjectStorage>,
    lose: Arc<AtomicBool>,
}
impl WorkspaceStoreCalls for LoseFirstRead {
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
    let central = Arc::new(ObjectStorage::new(
        f.root
            .path()
            .join("central")
            .join(format!("{}.sqlite", f.caller.organization_id.as_str())),
        Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
    ));
    let store = Arc::new(ObjectStorage::new(
        f.root
            .path()
            .join("workspace-stores")
            .join(format!("{}.sqlite", f.workspace)),
        Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    ));
    let runtime = WorkspaceRuntime::new(
        f.caller.organization_id.clone(),
        central.clone(),
        LoseFirstRead {
            store: store.clone(),
            lose: Arc::new(AtomicBool::new(true)),
        },
        f.clock.clone(),
        Arc::new(ClockIds::new(ArcClock(f.clock.clone()))),
    );
    let request = m::WorkspaceRequest {
        request_id: Some(id(891)),
        workspace_id: Some(f.workspace.clone()),
        operation: m::WorkspaceOperation::CreateArea {
            name: name("After read loss"),
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
                sql: "SELECT request_id FROM request_log WHERE request_id=?",
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
    central.close().await;
    store.close().await;
}
