//! The production thirty-day rule is crossed through the existing Clock capability.
#![cfg(feature = "conformance")]
use board_sync_protocol::{BoardReadPage, FleetFeedRead, FleetSnapshotCursor};
use collaboration_central_core::{CoreHost, FleetKind, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{CallFailure, NotSentReason, Uuid};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};
use message_board::{ChangeOutcome, RequestDecision};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const START: i64 = 1_792_281_600_000;
const THIRTY_DAYS_MS: i64 = 2_592_000_000;
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400c1";
#[derive(Clone)]
struct ClockIo(Arc<AtomicI64>);
impl Clock for ClockIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
impl IdSource for ClockIo {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}
impl WakeScheduler for ClockIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for ClockIo {
    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { last_seen: None },
        })
    }
}
struct Fixture {
    objects: ObjectStore<CoreHost<ClockIo>>,
    runtime: ObjectRuntime<FleetKind, TokioStorageHost<CoreHost<ClockIo>>, ClockIo>,
    io: ClockIo,
}
impl Fixture {
    async fn open(path: &std::path::Path) -> TestResult<Self> {
        let io = ClockIo(Arc::new(AtomicI64::new(START)));
        let objects = ObjectStore::open(path).await?;
        let receiver = ReceiverId::new(Uuid::now_v7())?;
        let object = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::fleet(
            Arc::new(TokioStorageHost::new(object)),
            io.clone(),
            receiver,
        )?;
        runtime.initialize().await?;
        Ok(Self {
            objects,
            runtime,
            io,
        })
    }
    async fn call(&self, kind: &str, request: serde_json::Value) -> TestResult<Vec<u8>> {
        self.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":Uuid::now_v7(),"step":"clock_read"},
            "kind":kind,"target":null,"request":request
        }))?).await.map_err(|error|format!("Fleet Clock case did not answer: {error:?}").into())
    }
    async fn commit(&self, operation: FleetOperation) -> TestResult {
        let request = FleetRequest {
            request_id: message_board::RequestId::try_from(Uuid::now_v7().to_string())?,
            workspace: message_board::WorkspaceId::try_from(
                "019f0000-0000-7000-8000-000000000c11".to_owned(),
            )?,
            actor: message_board::RecordedActor::Human {
                human_id: message_board::HumanId::try_from("clock owner".to_owned())?,
            },
            acting_for: None,
            protocol_version: message_board::ProtocolVersion::CURRENT,
            operation,
        };
        let bytes=self.call("central.fleet-metadata",json!({"request":request,"origin":{"installation":SERVICE,"machine":"clock fixture"}})).await?;
        if !matches!(
            serde_json::from_slice::<FleetRequestResult>(&bytes)?,
            FleetRequestResult::Decided(RequestDecision::Accepted {
                change: ChangeOutcome::Changed,
                ..
            })
        ) {
            return Err("Clock case did not reach a committed metadata mutation".into());
        }
        Ok(())
    }
    async fn feed(&self, after: u64) -> TestResult<BoardReadPage> {
        Ok(serde_json::from_slice(
            &self
                .call(
                    "central.fleet-read",
                    json!({"kind":"feed","value":{"after":after}}),
                )
                .await?,
        )?)
    }
    async fn snapshot(&self, cursor: Option<FleetSnapshotCursor>) -> TestResult<BoardReadPage> {
        Ok(serde_json::from_slice(
            &self
                .call(
                    "central.fleet-read",
                    json!({"kind":"snapshot","value":{"cursor":cursor}}),
                )
                .await?,
        )?)
    }
}

#[tokio::test]
async fn real_fleet_retention_keeps_the_prefix_before_thirty_days_and_expires_it_after_the_boundary()
-> TestResult {
    // Time is the only substituted capability. Canonical metadata, retention,
    // checked queries and response decoding use the real Fleet runtime/Turso.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.commit(FleetOperation::WorkspaceCreate {
            name:message_board::ResourceName::try_from("Thirty-day feed".to_owned())?,
            description:message_board::Description::try_from("Production boundary".to_owned())?,
        }).await?;
        for revision in 1..=4 {
            fixture.commit(FleetOperation::WorkspaceUpdate {
                name:message_board::ResourceName::try_from(format!("Initial prefix {revision}"))?,
                description:message_board::Description::try_from("Cross the decimal ordering boundary before expiry".to_owned())?,
            }).await?;
        }
        let BoardReadPage::FleetSnapshot {records,snapshot_at,..}=fixture.snapshot(None).await? else {
            return Err("initial snapshot did not expose its cut".into());
        };
        let first_record:message_board::Record=records.into_iter().next().ok_or("missing initial record")?.decode()?;
        let snapshot_cursor=FleetSnapshotCursor {snapshot_at,after:first_record.reference()};
        fixture.io.0.store(START+THIRTY_DAYS_MS-1,Ordering::SeqCst);
        fixture.commit(FleetOperation::WorkspaceUpdate {
            name:message_board::ResourceName::try_from("Before retention".to_owned())?,
            description:message_board::Description::try_from("Still inside the production interval".to_owned())?,
        }).await?;
        match fixture.feed(0).await? {
            BoardReadPage::FleetFeed(FleetFeedRead::Page {records,through,head,..})
                if records.len()==12 && through.get()==12 && head.get()==12=>{},
            _=>return Err("feed expired before the production thirty-day boundary".into()),
        }
        fixture.io.0.store(START+THIRTY_DAYS_MS+1,Ordering::SeqCst);
        fixture.commit(FleetOperation::WorkspaceUpdate {
            name:message_board::ResourceName::try_from("After retention".to_owned())?,
            description:message_board::Description::try_from("The first prefix must now be pruned".to_owned())?,
        }).await?;
        if !matches!(fixture.feed(0).await?,BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired {floor}) if floor.get()==10) {
            return Err("feed did not expose the exact expired prefix after thirty days".into());
        }
        match fixture.feed(10).await? {
            BoardReadPage::FleetFeed(FleetFeedRead::Page {records,through,head,..})
                if records.len()==4 && through.get()==14 && head.get()==14=>{},
            _=>return Err("remaining feed could not resume exactly at its retention floor".into()),
        }
        fixture.io.0.store(START+2*THIRTY_DAYS_MS,Ordering::SeqCst);
        fixture.commit(FleetOperation::WorkspaceUpdate {
            name:message_board::ResourceName::try_from("Second retention cycle".to_owned())?,
            description:message_board::Description::try_from("The original snapshot cut is now below the floor".to_owned())?,
        }).await?;
        if !matches!(fixture.snapshot(Some(snapshot_cursor)).await?,BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired {floor}) if floor.get()==12) {
            return Err("expired snapshot cut did not restart through its typed retention floor".into());
        }
        if !matches!(fixture.snapshot(None).await?,BoardReadPage::FleetSnapshot {snapshot_at,next:None,..} if snapshot_at.get()==16) {
            return Err("expired snapshot could not restart at the current head".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
