use super::*;
use central_storage::{StoreCall, StoreFailure, StoreReply};
use collaboration_central_core::WorkspaceRuntime;
use collaboration_central_tokio::ObjectStorage;
use fleet_runtime::ClockIds;
use fleet_runtime_ports::WorkspaceStores;
use fleet_runtime_ports::{Clock, SyncTime};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Clone)]
struct ArcClock(Arc<dyn Clock>);
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
    let due_batches = Arc::new(fleet_runtime::DueBatchState::default());
    let central = Arc::new(
        ObjectStorage::new(
            f.root
                .path()
                .join("central")
                .join(format!("{}.sqlite", f.caller.organization_id.as_str())),
            Arc::new(fleet_storage_schema::central_migrator()),
        )
        .with_engine_publication(
            f.shared_for_runtime(),
            fleet_runtime::engine_alarm_decider(due_batches.clone()),
        ),
    );
    let store = Arc::new(ObjectStorage::new(
        f.root
            .path()
            .join("workspace-stores")
            .join(format!("{}.sqlite", f.workspace)),
        Arc::new(fleet_storage_schema::workspace_migrator()),
    ));
    let runtime = WorkspaceRuntime::from_runtime(
        fleet_runtime::FleetRuntime::open(
            f.caller.organization_id.clone(),
            central.clone(),
            LoseFirstRead {
                store: store.clone(),
                lose: Arc::new(AtomicBool::new(true)),
            },
            f.shared_for_runtime(),
            Arc::new(ClockIds::new(ArcClock(f.shared_for_runtime()))),
            due_batches,
            Arc::new(|_| {}),
        ),
        Ok(None),
    )
    .unwrap();
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
