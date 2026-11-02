//! Published schemas validate emitted S6 values; runtime custody has separate real-path proofs.
use super::emitted_result_schema_tests::{
    advertised_output_validator, validated_structured_content,
};
use crate::mcp_server::success_result;
use collaboration_protocol::{AutomationPage, FiringSnapshot, ScheduleSnapshot};
use serde_json::{Value, json};

fn firing_fixture() -> Result<FiringSnapshot, serde_json::Error> {
    serde_json::from_value(json!({
        "firingId":collaboration_protocol::FiringId::generate(),
        "automationId":collaboration_protocol::AutomationId::generate(),
        "dueAt":0,"revision":collaboration_protocol::ChangeId::generate(),
        "content":{"contentId":collaboration_protocol::ContentId::generate(),
            "revisionId":collaboration_protocol::RevisionId::generate(),"digest":"0".repeat(64)},
        "outcome":{"state":"held"},"reportSeq":null,"batchId":null
    }))
}

#[test]
fn firing_tools_accept_shared_values_and_reject_missing_identity_or_off_grid_due_time() {
    let firing = firing_fixture().expect("shared Firing fixture");
    let validator = advertised_output_validator("firing_show");
    let encoded = validated_structured_content(&validator, "firing_show", success_result(&firing));
    assert_eq!(encoded["reportSeq"], Value::Null);
    assert_eq!(encoded["batchId"], Value::Null);
    for field in ["firingId", "reportSeq", "batchId"] {
        let mut missing = encoded.clone();
        missing
            .as_object_mut()
            .expect("Firing object")
            .remove(field);
        assert!(
            !validator.is_valid(&missing),
            "Firing lost required {field}"
        );
    }
    let mut off_grid = encoded;
    off_grid["dueAt"] = json!(1);
    assert!(
        !validator.is_valid(&off_grid),
        "Firing schema rounded an off-grid due time"
    );
    let page = AutomationPage {
        records: vec![firing],
        next_cursor: Some("actual-opaque-token".to_owned()),
    };
    let validator = advertised_output_validator("firing_list");
    let encoded = validated_structured_content(&validator, "firing_list", success_result(&page));
    assert_eq!(encoded["nextCursor"], "actual-opaque-token");
}

#[test]
fn central_schedule_schema_omits_local_occupancy_and_requires_nullable_due_time() {
    let local: ScheduleSnapshot = serde_json::from_value(json!({
        "scheduleId":collaboration_protocol::ScheduleId::generate(),
        "changeId":collaboration_protocol::ChangeId::generate(),
        "definition":{"instructionId":collaboration_protocol::InstructionId::generate(),
            "timing":{"kind":"interval","seconds":600},"enabled":false,
            "destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
            "model":"gpt-6-luna","effort":"low"},
        "importedContinuity":{"kind":"none"},"anchorAt":"2026-11-01T12:00:00Z",
        "nextDueAt":null,"activeRunId":null,"waitingRunId":null,
        "createdAt":"2026-11-01T12:00:00Z","updatedAt":"2026-11-01T12:00:00Z"
    }))
    .expect("shared retained Schedule fixture");
    let central = collaboration_service::CentralScheduleSnapshot::from(local.clone());
    let validator = advertised_output_validator("schedule_create");
    let encoded =
        validated_structured_content(&validator, "schedule_create", success_result(&central));
    assert_eq!(encoded["nextDueAt"], Value::Null);
    for field in ["activeRunId", "waitingRunId"] {
        assert!(
            encoded.get(field).is_none(),
            "central catalog claimed local occupancy"
        );
        let mut invented = encoded.clone();
        invented
            .as_object_mut()
            .expect("central Schedule object")
            .insert(field.to_owned(), Value::Null);
        assert!(
            !validator.is_valid(&invented),
            "central schema accepted invented {field}"
        );
    }
    let mut missing_due = encoded;
    missing_due
        .as_object_mut()
        .expect("central Schedule object")
        .remove("nextDueAt");
    assert!(
        !validator.is_valid(&missing_due),
        "nullable nextDueAt became optional"
    );
    let validator = advertised_output_validator("schedule_show");
    let local = collaboration_service::ScheduleReadResponse::Local { value: local };
    let encoded = validated_structured_content(&validator, "schedule_show", success_result(&local));
    assert_eq!(encoded["value"]["activeRunId"], Value::Null);
    assert_eq!(encoded["value"]["waitingRunId"], Value::Null);
    let central = collaboration_service::ScheduleReadResponse::Schedule { value: central };
    validated_structured_content(&validator, "schedule_show", success_result(&central));
}

#[test]
fn central_wake_wait_schema_preserves_held_firing_pending_and_faulted_evidence() {
    use collaboration_service::{CentralWakeWaitOutcome, CentralWakeWaitResult, WakeWaitResponse};
    let firing = firing_fixture().expect("shared Firing fixture");
    let wakeup_id = firing
        .automation_id
        .as_str()
        .to_owned()
        .try_into()
        .expect("Wake UUID");
    let validator = advertised_output_validator("wake_wait_until_first_fire");
    let value = WakeWaitResponse::Central {
        value: CentralWakeWaitResult {
            wakeup_id,
            outcome: CentralWakeWaitOutcome::Fired {
                firing: Box::new(firing.clone()),
            },
        },
    };
    let encoded = validated_structured_content(
        &validator,
        "wake_wait_until_first_fire",
        success_result(&value),
    );
    assert_eq!(
        encoded["value"]["outcome"]["firing"],
        serde_json::to_value(&firing).expect("Firing wire")
    );
    assert_eq!(
        encoded["value"]["outcome"]["firing"]["outcome"]["state"],
        "held"
    );
    assert!(encoded["value"].get("cursor").is_none());
    let pending = WakeWaitResponse::Central {
        value: CentralWakeWaitResult {
            wakeup_id: firing
                .automation_id
                .as_str()
                .to_owned()
                .try_into()
                .expect("Wake UUID"),
            outcome: CentralWakeWaitOutcome::Pending { firing_id: None },
        },
    };
    let encoded = validated_structured_content(
        &validator,
        "wake_wait_until_first_fire",
        success_result(&pending),
    );
    assert!(encoded["value"]["outcome"].get("firingId").is_none());
    let fault: collaboration_protocol::FaultedAutomationSnapshot = serde_json::from_value(json!({
        "automationId":firing.automation_id,"kind":"wake","revision":firing.revision,
        "fault":{"error":{"kind":"invalidRecord","field":"definition_json"},"faultedAt":"2026-11-01T12:00:00Z"}
    })).expect("shared fault fixture");
    let value = WakeWaitResponse::Faulted {
        message: "Stored Wake fault".to_owned(),
        fault: fault.clone(),
    };
    let encoded = validated_structured_content(
        &validator,
        "wake_wait_until_first_fire",
        success_result(&value),
    );
    assert_eq!(encoded["kind"], "faulted");
    assert_eq!(
        encoded["fault"],
        serde_json::to_value(fault).expect("unmodified shared fault")
    );
}

#[test]
fn suspended_read_and_mutation_schemas_preserve_shared_configuration_and_required_nulls() {
    use collaboration_protocol::{AutomationKind, SuspendedAutomationSnapshot};
    use collaboration_service::{
        CatalogAutomationResult, CatalogMutationResult, ScheduleMutationResponse,
        ScheduleReadResponse, WakeMutationResponse, WakeReadResponse, WakeWaitResponse,
    };
    let target = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"suspended-schema-target"});
    for kind in [AutomationKind::Schedule, AutomationKind::Wake] {
        let configuration = match kind {
            AutomationKind::Schedule => json!({"kind":"schedule","value":{
                "instructionId":collaboration_protocol::InstructionId::generate(),"enabled":false,
                "destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":null,"effort":null
            }}),
            AutomationKind::Wake => json!({"kind":"wake","value":{
                "configuration":{"target":target,"origin":{"kind":"agent","sender":target},
                    "delivery":"auto","generationGuard":null,"expiresAt":"2026-11-02T12:00:00Z","state":"active"},
                "createdBy":{"kind":"owner"},
                "content":{"contentId":collaboration_protocol::ContentId::generate(),
                    "revisionId":collaboration_protocol::RevisionId::generate(),"digest":"0".repeat(64)}
            }}),
        };
        let suspended: SuspendedAutomationSnapshot = serde_json::from_value(json!({
            "automationId":collaboration_protocol::AutomationId::generate(),"kind":kind,
            "revision":collaboration_protocol::ChangeId::generate(),
            "legacyTiming":{"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"},
            "configuration":configuration,
            "header":{"anchorAt":"2026-11-01T12:00:00Z","createdAt":"2026-11-01T12:00:00Z",
                "updatedAt":"2026-11-01T12:00:00Z","importedContinuity":null}
        }))
        .expect("shared suspended configuration");
        let shared = serde_json::to_value(&suspended).expect("shared suspended wire");
        let mutation = CatalogMutationResult {
            automation: CatalogAutomationResult::Suspended(suspended.clone()),
            already_sent: Vec::new(),
        };
        let (show, list, reads, mutation_tool, mutation) = match kind {
            AutomationKind::Schedule => {
                let read = ScheduleReadResponse::Suspended {
                    message: "Repair timing".into(),
                    suspended: suspended.clone(),
                };
                (
                    "schedule_show",
                    "schedule_list",
                    vec![serde_json::to_value(read).expect("Schedule read")],
                    "schedule_disable",
                    serde_json::to_value(ScheduleMutationResponse::Central(mutation))
                        .expect("Schedule mutation"),
                )
            }
            AutomationKind::Wake => {
                let read = WakeReadResponse::Suspended {
                    message: "Repair timing".into(),
                    suspended: suspended.clone(),
                };
                (
                    "wake_show",
                    "wake_list",
                    vec![
                        serde_json::to_value(read).expect("Wake read"),
                        serde_json::to_value(WakeWaitResponse::Suspended {
                            message: "Repair timing".into(),
                            suspended: Box::new(suspended),
                        })
                        .expect("Wake wait"),
                    ],
                    "wake_pause",
                    serde_json::to_value(WakeMutationResponse::Central(mutation))
                        .expect("Wake mutation"),
                )
            }
        };
        for (index, read) in reads.iter().enumerate() {
            let tool = if index == 0 {
                show
            } else {
                "wake_wait_until_first_fire"
            };
            let validator = advertised_output_validator(tool);
            let encoded = validated_structured_content(&validator, tool, success_result(read));
            assert_eq!(encoded["suspended"], shared);
            assert!(encoded.get("value").is_none());
            let mut missing = encoded;
            missing["suspended"]["header"]
                .as_object_mut()
                .expect("header")
                .remove("importedContinuity");
            assert!(
                !validator.is_valid(&missing),
                "{tool} made required nullable continuity optional"
            );
        }
        let page = AutomationPage {
            records: vec![reads[0].clone()],
            next_cursor: None,
        };
        validated_structured_content(
            &advertised_output_validator(list),
            list,
            success_result(&page),
        );
        let encoded = validated_structured_content(
            &advertised_output_validator(mutation_tool),
            mutation_tool,
            success_result(&mutation),
        );
        assert_eq!(encoded["value"]["automation"]["kind"], "suspended");
        assert_eq!(encoded["value"]["automation"]["value"], shared);
    }
}
