use super::*;
use acp_client_runtime::{
    AutoPermissionMode, ModelMetadataSource, PreparedCompact, ProviderBehavior, ProviderError,
    ProviderFacts, ProviderInitializationAuthentication, ProviderUpdate,
};
use acp_provider_adapters::{CursorAcpBehavior, StandardAcpBehavior};
use collaboration_contracts::SessionEventHub;
use collaboration_protocol::ProviderSessionInspectRequest;
use provider_session_machines::ModelReasoningMetadata;
use provider_session_machines::{ProviderSettingKind, session_owner::ObservationCause};
use session_event_model::{PromptContent, ProviderSubmission};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[tokio::test]
async fn a_ready_host_restore_publishes_the_setters_catalogue_and_keeps_saved_setup() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let mut store = rig.store().await;
    let mut record = store
        .session_record(&rig.target)
        .await
        .expect("read session setup")
        .expect("seeded session");
    record.resume.requested_mode = Some("plan".to_owned());
    store
        .record_session(&record)
        .await
        .expect("seed saved Plan mode");
    drop(store);
    let host = rig.open_host("restored-in-auto").await;

    rig.assert_loads(&host).await;
    assert_eq!(
        rig.wire_receipt()["modeRequests"],
        serde_json::json!([{"sessionId":rig.target.session_id,"value":"plan"}]),
    );
    let shown = host
        .inspect_session(ProviderSessionInspectRequest {
            target: rig.target.clone(),
        })
        .await
        .expect("read active catalogue");
    assert_eq!(
        shown
            .settings_catalog
            .expect("ready session settings")
            .current_mode
            .as_deref(),
        Some("plan"),
    );
    let owner = host
        .session_owner_for(&rig.target)
        .expect("ready session owner");
    assert_eq!(
        owner
            .catalog()
            .expect("active ACP catalogue")
            .effective_settings()
            .mode
            .as_deref(),
        Some("plan"),
        "the ready Host routes subsequent work with the setter's catalogue",
    );
    let saved = rig
        .store()
        .await
        .session_record(&rig.target)
        .await
        .expect("read stored setup after load")
        .expect("saved session remains");
    assert_eq!(saved.resume, record.resume);
    host.shutdown().await.expect("fixture shutdown");
}

#[derive(Clone, Copy)]
enum RestoreModeReplyTiming {
    BeforeDeadline,
    AtDeadline,
}

struct RestoreModeReplyClock {
    manual: Arc<ManualClock>,
    wire_receipt: PathBuf,
    timing: RestoreModeReplyTiming,
    decoded_mode_reply: AtomicBool,
}

impl ProviderBehavior for RestoreModeReplyClock {
    fn initialization_authentication(&self) -> ProviderInitializationAuthentication {
        StandardAcpBehavior.initialization_authentication()
    }

    fn can_reuse_setting_confirmation(&self, kind: ProviderSettingKind) -> bool {
        CursorAcpBehavior.can_reuse_setting_confirmation(kind)
    }

    fn model_metadata_source(&self) -> ModelMetadataSource {
        let mode_was_written = std::fs::read(&self.wire_receipt)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|wire| {
                wire.get("modeRequests")
                    .and_then(|rows| rows.as_array())
                    .map(|rows| !rows.is_empty())
            })
            .unwrap_or(false);
        if mode_was_written
            && !self.decoded_mode_reply.swap(true, Ordering::Relaxed)
            && matches!(self.timing, RestoreModeReplyTiming::AtDeadline)
        {
            // The fixture records its mode request before replying. Catalogue
            // decoding and completion send run in one poll on this test's
            // current-thread runtime, so the deadline and reply are both
            // ready when the owner next runs.
            self.manual
                .advance(Duration::from_secs(30))
                .expect("reach the Mode deadline while decoding its reply");
        }
        StandardAcpBehavior.model_metadata_source()
    }

    fn decode_model_metadata(&self, value: &serde_json::Value) -> ModelReasoningMetadata {
        StandardAcpBehavior.decode_model_metadata(value)
    }

    fn auto_permission_mode(&self) -> AutoPermissionMode {
        CursorAcpBehavior.auto_permission_mode()
    }

    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        StandardAcpBehavior.observe(update)
    }

    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        StandardAcpBehavior.prepare(submission)
    }

    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        StandardAcpBehavior.compact()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_restore_mode_deadline_publishes_load_catalogue_before_the_late_reply() {
    assert_restore_mode_reply(
        RestoreModeReplyTiming::AtDeadline,
        "ask",
        Some(ObservationCause::NoAnswer),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_restore_mode_reply_accepted_before_the_deadline_can_grant_auto() {
    assert_restore_mode_reply(RestoreModeReplyTiming::BeforeDeadline, "agent", None).await;
}

async fn assert_restore_mode_reply(
    timing: RestoreModeReplyTiming,
    initial_mode: &str,
    expected_cause: Option<ObservationCause>,
) {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let scratch = rig.root.path().join("scratch");
    std::fs::create_dir(&scratch).expect("saved scratch directory");
    let mut store = rig.store().await;
    let mut record = store
        .session_record(&rig.target)
        .await
        .expect("read session setup")
        .expect("seeded session");
    record.resume.requested_mode = Some("auto".to_owned());
    record.resume.scratch_folder = Some(
        ProviderWorkingDirectory::try_from(scratch.display().to_string())
            .expect("saved scratch path"),
    );
    store
        .record_session(&record)
        .await
        .expect("seed saved Auto");
    drop(store);
    let adapter = Arc::new(RestoreModeReplyClock {
        manual: rig.clock.clone(),
        wire_receipt: rig.root.path().join("wire.json"),
        timing,
        decoded_mode_reply: AtomicBool::new(false),
    });
    let (host, hub) = rig
        .open_host_with_fixture_arguments(&[], adapter.clone())
        .await;

    let restore = OperationId::generate();
    host.load(rig.load_request(restore.clone()))
        .await
        .expect("restore admitted");
    let owner = host.session_owner_for(&rig.target).expect("loaded owner");
    let snapshot = owner
        .snapshots()
        .wait_for(|snapshot| snapshot.last_restore.is_some())
        .await
        .expect("owner settles the restore")
        .clone();
    assert!(adapter.decoded_mode_reply.load(Ordering::Relaxed));
    let outcome = snapshot
        .last_restore
        .expect("settled restore")
        .observed
        .get(&ProviderSettingKind::Mode)
        .expect("mode evidence")
        .clone();
    assert_eq!(outcome.cause, expected_cause);
    let hub_target: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&rig.target).expect("target JSON"))
            .expect("hub target");
    let first_mode = loop {
        let attachment = hub
            .attach(hub_target.clone())
            .await
            .expect("attach to restored history");
        if let Some(mode) = attachment.snapshot.into_iter().find_map(|entry| {
            if let session_event_model::SessionEvent::SettingsChanged { settings } = entry.event {
                settings.mode
            } else {
                None
            }
        }) {
            break mode;
        }
        tokio::task::yield_now().await;
    };
    assert_eq!(
        first_mode, initial_mode,
        "Host preparation uses only the catalogue accepted by the owner"
    );

    owner
        .snapshots()
        .wait_for(|snapshot| {
            snapshot
                .projection
                .settings
                .as_ref()
                .is_some_and(|settings| settings.mode.as_deref() == Some("agent"))
        })
        .await
        .expect("the current live mode reply is accepted");
    let active = owner.catalog().expect("active catalogue");
    assert_eq!(
        active.effective_settings().mode.as_deref(),
        Some("agent"),
        "the same live run accepts its still-current mode observation",
    );
    assert!(matches!(
        host.wait(wait_request(restore))
            .await
            .expect("the Host publishes restore settlement")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { .. }
        }
    ));
    let edit = serde_json::json!({"kind":"edit","locations":[{"path":scratch.join("note.txt")}]});
    let input = OperationId::generate();
    host.prompt(rig.prompt_request(input.clone(), &edit.to_string()))
        .await
        .expect("prompt admitted");
    host.wait(wait_request(input)).await.expect("edit settles");
    assert_eq!(
        rig.wire_receipt()
            .pointer("/permissionAnswers/0/answer/outcome"),
        Some(&serde_json::json!({"outcome":"selected","optionId":"allow"})),
        "the active run has the requested Auto and an owner-confirmed mode",
    );
    host.shutdown().await.expect("fixture shutdown");
}
