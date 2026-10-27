#![cfg(feature = "conformance")]

use collaboration_central_core::{CoreHost, ObjectAddress, PlatformIo};
use durable_steps::{CallFailure, CallId, CallTarget, Host, OperationId, StepName};
use fleet_sync_lease::CopyRead;
use fleet_sync_protocol::{CopyStatusAnswer, DatabaseId, ReceiverId};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::sync::{Arc, Mutex};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type ObservedCall = (ObjectAddress, Vec<u8>);

#[derive(Clone)]
struct RecordingIo {
    calls: Arc<Mutex<Vec<ObservedCall>>>,
    answer: Vec<u8>,
}

impl Clock for RecordingIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for RecordingIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for RecordingIo {
    async fn wake_at(&self, _at: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for RecordingIo {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push((destination, body));
        Ok(self.answer.clone())
    }
}

fn receiver() -> Result<ReceiverId, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ReceiverId::try_from(
        "019f0000-0000-7000-8000-000000000010".to_owned(),
    )?)
}

fn identity() -> Result<CallId, Box<dyn std::error::Error + Send + Sync>> {
    Ok(CallId {
        operation: OperationId::try_from("019f0000-0000-7000-8000-000000000011".to_owned())?,
        step: StepName::new("read_copy")?,
    })
}

fn copy_read() -> Result<CopyRead, Box<dyn std::error::Error + Send + Sync>> {
    Ok(CopyRead {
        copy: receiver()?,
        database: DatabaseId::try_from("019f0000-0000-7000-8000-000000000012".to_owned())?,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn registry_call_carries_its_bound_face_target_and_original_call_identity() -> TestResult {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = CoreHost::conformance_registry(
        RecordingIo {
            calls: calls.clone(),
            answer: serde_json::to_vec(&CopyStatusAnswer::NotInitialized)?,
        },
        receiver()?,
    );

    let result = host.dispatch(copy_read()?, (), identity()?).await?;

    assert_eq!(result, CopyStatusAnswer::NotInitialized);
    let captured = calls.lock().map_err(|_| "observation lock poisoned")?;
    let [(destination, bytes)] = captured.as_slice() else {
        return Err("expected exactly one object call".into());
    };
    assert_eq!(
        *destination,
        ObjectAddress::Copy(fleet_sync_protocol::DatabaseId::try_from(
            "019f0000-0000-7000-8000-000000000012".to_owned()
        )?)
    );
    let envelope: serde_json::Value = serde_json::from_slice(bytes)?;
    assert_eq!(
        envelope,
        serde_json::json!({
        "source": {"kind": "registry"}, "sourceFace": "registry",
        "authority": {"kind":"object"},
        "destination": {"kind": "copy", "value": "019f0000-0000-7000-8000-000000000012"},
            "destinationFace": "copy",
            "identity": {"operation": "019f0000-0000-7000-8000-000000000011", "step": "read_copy"},
            "kind": "fleet-sync.copy-status",
            "target": {"copy": "019f0000-0000-7000-8000-000000000010", "database": "019f0000-0000-7000-8000-000000000012"},
            "request": null
        })
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_unregistered_call_is_refused_before_dispatch() -> TestResult {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = CoreHost::conformance_registry(
        RecordingIo {
            calls: calls.clone(),
            answer: Vec::new(),
        },
        receiver()?,
    );

    let result = host.dispatch(UnregisteredTarget, (), identity()?).await;

    assert!(matches!(result, Err(CallFailure::NotSent { .. })));
    assert!(
        calls
            .lock()
            .map_err(|_| "observation lock poisoned")?
            .is_empty()
    );
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UnregisteredTarget;

impl CallTarget for UnregisteredTarget {
    const KIND: &'static str = "conformance.unregistered";
    type Request = ();
    type Answer = ();
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_unreadable_reply_after_dispatch_is_an_unknown_outcome() -> TestResult {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = CoreHost::conformance_registry(
        RecordingIo {
            calls: calls.clone(),
            answer: b"{".to_vec(),
        },
        receiver()?,
    );

    let result = host.dispatch(copy_read()?, (), identity()?).await;

    assert!(matches!(result, Err(CallFailure::AnswerLost)));
    assert_eq!(
        calls.lock().map_err(|_| "observation lock poisoned")?.len(),
        1
    );
    Ok(())
}
