use super::*;
use agent_automation::{CapturedRunInputs, RouteEffectEvidence, RunId, RunPhase};
use collaboration_contracts::RunEvidenceDisposition;
use collaboration_contracts::RunEvidenceSink;
use collaboration_contracts::ScheduledRunExecution;
use collaboration_protocol::CodexGeneration;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct CountingStopSink {
    writes: AtomicUsize,
}

impl RunEvidenceSink for CountingStopSink {
    fn record(
        &self,
        _: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, RunEvidenceDisposition> {
        Box::pin(async { Ok(RunEvidenceDisposition::Recorded { timing: None }) })
    }

    fn record_stop_intent(&self) -> DeliveryFuture<'_, RunEvidenceDisposition> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(RunEvidenceDisposition::Recorded { timing: None }) })
    }
}

#[tokio::test]
async fn unavailable_stop_connection_never_calls_intent_sink()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = "00000000-0000-4000-8000-000000000001";
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":service,"endpointId":"codex-local"},
        "sessionId":"worker-thread"
    }))?;
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?;
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadTurnsList",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
        &bundle,
    )?);
    let gate = crate::NativeGenerationGate::default();
    let missing_socket = std::env::temp_dir().join(format!(
        "absent-stop-{}.sock",
        agent_automation::OperationId::generate().as_str()
    ));
    gate.activate(generation.clone(), missing_socket, Some(schemas))?;
    let route = CodexAppServerScheduledRuns::new(
        std::sync::Arc::new(
            router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
                1792432800000,
            ))
            .unwrap(),
        ),
        NativeControlBackend {
            endpoint: target.endpoint.clone(),
            gate,
            codex_home: std::env::temp_dir(),
        },
        std::sync::Arc::new(crate::UnmaterializedThreadHolder::new()),
    );
    let recorded = serde_json::from_value(json!({
        "kind":"codexAppServer","target":target,"generation":generation,
        "clientUserMessageId":"run-correlation","nativeTurnId":"worker-turn",
        "nativeSubmissionId":null,"allocation":"notRequested","resume":"notRequested",
        "submission":"accepted","cessation":"unconfirmed"
    }))?;
    let inputs: CapturedRunInputs<SessionRef, EndpointRef> = serde_json::from_value(json!({
        "scheduleChangeId":agent_automation::ChangeId::generate(),
        "instructionRevisionId":agent_automation::RevisionId::generate(),
        "instructionText":"Check work",
        "continuity":{"kind":"none"},
        "executionConfiguration":{
            "destination":{"kind":"ownedThread","target":target,"cwd":"/tmp"},
            "executionTimeoutSeconds":120,"model":"fixture-model","effort":"medium"
        }
    }))?;
    let sink = CountingStopSink {
        writes: AtomicUsize::new(0),
    };
    let result = route
        .request_stop(
            RunObservationContext {
                run_id: RunId::generate(),
                phase: RunPhase::Executing,
                recorded,
                inputs,
            },
            &sink,
        )
        .await?;
    if !matches!(result, StopRequestOutcome::Unknown) || sink.writes.load(Ordering::SeqCst) != 0 {
        return Err("unreachable client persisted a stop intent".into());
    }
    Ok(())
}
