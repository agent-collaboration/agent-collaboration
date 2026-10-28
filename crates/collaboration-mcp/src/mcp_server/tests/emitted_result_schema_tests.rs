//! The advertised output schemas accept the results tools build from real domain values:
//! classified Codex session failures, lifecycle records decoded from the journal's wire form,
//! and provider and peer run evidence decoded through `RunSnapshot`'s validation. The
//! hand-written samples for every tool live in `schema_tests.rs`.
use super::*;
use collaboration_service::collaboration_application::{
    NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage,
};

pub(super) fn advertised_output_validator(tool_name: &str) -> jsonschema::Validator {
    let schema = CollaborationMcpServer::catalog_only()
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .and_then(|tool| tool.output_schema)
        .unwrap_or_else(|| panic!("{tool_name} advertises an output schema"));
    jsonschema::validator_for(&Value::Object((*schema).clone()))
        .unwrap_or_else(|error| panic!("{tool_name} output validator: {error}"))
}

#[test]
fn forwarded_post_outcomes_match_both_advertised_post_schemas() {
    for tool in ["thread_create", "thread_post"] {
        let validator = advertised_output_validator(tool);
        for value in [
            serde_json::json!({"kind":"notHeld"}),
            serde_json::json!({"kind":"holderUnreachable","decision":{"lastSeen":"2026-10-22T00:00:00Z"}}),
            serde_json::json!({"kind":"holderUnreachable","decision":{"lastSeen":null}}),
            serde_json::json!({"kind":"unanswered"}),
            serde_json::json!({"kind":"overloaded"}),
        ] {
            let reply: message_board::public_replies::WriteReply =
                serde_json::from_value(serde_json::json!({"result":value,"reader":null})).unwrap();
            let encoded = validated_structured_content(
                &validator,
                tool,
                super::super::success_result(&reply),
            );
            assert_eq!(encoded["result"], value);
        }
        assert!(!validator.is_valid(&serde_json::json!({"result":{"kind":"holderUnreachable","decision":{"lastSeen":42}},"reader":null})));
        assert!(
            !validator
                .is_valid(&serde_json::json!({"result":{"kind":"unknownForward"},"reader":null}))
        );
    }
}

#[test]
fn accepted_pending_project_creation_is_a_success_with_a_required_project_identity() {
    let project: message_board::ProjectId = FIXTURE_PUSH_ID.to_owned().try_into().unwrap();
    let reply = message_board::public_replies::WriteReply {
        result: message_board::public_replies::RequestResult::ProjectCreationPending {
            project: project.clone(),
        },
        reader: None,
    };
    let validator = advertised_output_validator("project_create");
    let mut encoded = validated_structured_content(
        &validator,
        "project_create",
        super::super::success_result(&reply),
    );
    assert_eq!(
        encoded["result"],
        serde_json::json!({
            "kind":"projectCreationPending", "decision":{"project":project},
        })
    );
    encoded["result"]["decision"]
        .as_object_mut()
        .unwrap()
        .remove("project");
    assert!(
        !validator.is_valid(&encoded),
        "accepted pending reply lost its durable project identity"
    );
}

pub(super) fn validated_structured_content(
    validator: &jsonschema::Validator,
    tool_name: &str,
    result: rmcp::model::CallToolResult,
) -> Value {
    let structured = result
        .structured_content
        .unwrap_or_else(|| panic!("{tool_name} result is structured"));
    if let Err(error) = validator.validate(&structured) {
        panic!("{tool_name} advertised schema rejects its own result: {error}; {structured}");
    }
    structured
}

#[test]
fn session_inspect_and_rename_schemas_accept_every_classified_failure_they_emit() {
    let native_rejection = |stage, message: &str, reason, next_action, native_code| {
        NativeSessionFailure::NativeRejected {
            stage,
            message: message.to_owned(),
            reason,
            next_action,
            native_code,
        }
    };
    for (tool_name, stage, possible_effect) in [
        (
            "session_inspect",
            NativeSessionStage::Inspect,
            OperationEffect::None,
        ),
        (
            "session_rename",
            NativeSessionStage::Rename,
            OperationEffect::Unknown,
        ),
    ] {
        // Arrange: one failure per class the classifier hands these tools.
        let validator = advertised_output_validator(tool_name);
        let mut failures = vec![
            native_rejection(
                stage,
                "thread has an active turn and is busy",
                collaboration_protocol::DeliveryRejectionReason::Busy,
                collaboration_protocol::DeliveryNextAction::UseDeliverySteer,
                None,
            ),
            native_rejection(
                stage,
                "Native control operation failed",
                collaboration_protocol::DeliveryRejectionReason::Unknown,
                collaboration_protocol::DeliveryNextAction::RetryLater,
                Some(-32099),
            ),
            native_rejection(
                stage,
                "thread 019f0000-0000-7000-8000-000000000001 already has an active writer",
                collaboration_protocol::DeliveryRejectionReason::HeldByAnotherClient,
                collaboration_protocol::DeliveryNextAction::MessageFromHoldingCodexClient,
                None,
            ),
            NativeSessionFailure::Refused {
                kind: NativeSessionFailureKind::Unavailable,
                stage,
                message: "Native control operation failed".to_owned(),
            },
        ];
        if stage == NativeSessionStage::Rename {
            failures.push(NativeSessionFailure::NameMismatch {
                requested: "Review".to_owned(),
                effective: "Old name".to_owned(),
            });
        }

        for failure in failures {
            let classification = serde_json::to_value(&failure).expect("failure encodes");

            // Act
            let result =
                super::super::application_result::<Value, _>(Err(failure), possible_effect);

            // Assert: the schema admits the error, and the error keeps the classification
            // (reason, corrective action, native code, requested and echoed names).
            assert_eq!(result.is_error, Some(true), "{tool_name} reports an error");
            let structured = validated_structured_content(&validator, tool_name, result);
            assert_eq!(
                structured["data"], classification,
                "{tool_name} error keeps the classified failure"
            );
        }
    }
}

#[test]
fn journal_read_schema_accepts_a_page_of_decoded_flattened_lifecycle_records() {
    // Arrange: a page decoded from the journal's wire form, so each record is the flattened
    // observation the journal actually stores.
    let validator = advertised_output_validator("journal_read");
    let id = FIXTURE_SERVICE_ID;
    let page: collaboration_protocol::JournalPage = serde_json::from_value(serde_json::json!({
        "bounds":{"journalId":id,"earliestSequence":1,"lastSequence":1},
        "records":[{
            "journalId":id,"sequence":1,"schemaVersion":1,"observedAt":"2026-09-05T12:00:00Z",
            "source":"observerLifecycle",
            "scope":{"endpoint":{"serviceId":id,"endpointId":"codex-local"},"generation":null,"observerId":id},
            "subject":{"kind":"backend"},"change":{"kind":"coverageLost"}
        }],
        "next":{"journalId":id,"sequence":1},"caughtUp":true
    }))
    .expect("journal page decodes");

    // Act
    let result = super::super::success_result(&page);

    // Assert
    let structured = validated_structured_content(&validator, "journal_read", result);
    assert_eq!(structured["records"][0]["source"], "observerLifecycle");
}

#[test]
fn run_show_schema_accepts_provider_and_peer_run_evidence() {
    // Arrange: one provider run still executing and one finished peer write, each decoded
    // through `RunSnapshot`'s own validation.
    let validator = advertised_output_validator("run_show");
    let endpoint = serde_json::json!({"serviceId":FIXTURE_SERVICE_ID,"endpointId":"claude-local"});
    let start = "2026-09-24T00:00:00Z";
    let deadline = "2026-09-24T00:02:00Z";
    let timing = serde_json::json!({
        "dispatchStartedAt":start,"effectiveTimeoutSeconds":120,"deadlineAt":deadline
    });
    let inputs = |target: &Value, instruction: &str, sequence: u8| {
        serde_json::json!({
            "scheduleChangeId":format!("019f0000-0000-7000-8000-0000000001{sequence:02x}"),
            "instructionRevisionId":format!("019f0000-0000-7000-8000-0000000002{sequence:02x}"),
            "instructionText":instruction,
            "continuity":{"kind":"none"},
            "executionConfiguration":{
                "destination":{"kind":"ownedThread","target":target,"cwd":"/isolated-fixture"},
                "executionTimeoutSeconds":120,"model":"fixture-model","effort":"medium"
            }
        })
    };
    let provider_target = serde_json::json!({"endpoint":endpoint,"sessionId":"provider-worker"});
    let operation = "019f0000-0000-7000-8000-000000000301";
    let provider: collaboration_protocol::RunSnapshot = serde_json::from_value(serde_json::json!({
        "runId":"019f0000-0000-7000-8000-000000000401",
        "scheduleId":"019f0000-0000-7000-8000-000000000501",
        "dueAt":start,
        "state":{"kind":"executing","inputs":inputs(&provider_target, "Check provider job", 1),
            "execution":{"kind":"providerAcp","target":provider_target,"operationId":operation,
                "startedAt":start,"deadlineAt":deadline,"effectiveTimeoutSeconds":120}},
        "executionEvidence":{
            "route":{"kind":"providerAcp","bindingId":"fixture-binding",
                "generation":{"serviceEpoch":FIXTURE_SERVICE_ID,"generation":1},
                "target":provider_target,"operationId":operation,"submission":"accepted"},
            "timing":timing,
            "acceptance":{"outcome":{"kind":"started"},"reachability":"providerAcp",
                "client":{"kind":"providerAcp","operationId":operation}}
        },
        "summary":null
    }))
    .expect("provider run decodes");
    let peer_target = serde_json::json!({"endpoint":endpoint,"sessionId":"peer-worker"});
    let peer: collaboration_protocol::RunSnapshot = serde_json::from_value(serde_json::json!({
        "runId":"019f0000-0000-7000-8000-000000000402",
        "scheduleId":"019f0000-0000-7000-8000-000000000502",
        "dueAt":start,
        "state":{"kind":"finished","inputs":inputs(&peer_target, "Notify peer", 2),
            "execution":{"kind":"claudeCodePeer","target":peer_target,"writtenAt":start},
            "outcome":{"kind":"peerMessageWritten","explanation":"written"},
            "summaryRunId":null},
        "executionEvidence":{
            "route":{"kind":"claudeCodePeer","sessionId":"peer-worker","write":"written"},
            "timing":timing,"acceptance":null
        },
        "summary":null
    }))
    .expect("peer run decodes");

    // Act
    let provider_result = super::super::success_result(&provider);
    let peer_result = super::super::success_result(&peer);

    // Assert
    let mut provider_structured =
        validated_structured_content(&validator, "run_show", provider_result);
    validated_structured_content(&validator, "run_show", peer_result);
    provider_structured["executionEvidence"]["native"] = serde_json::json!({});
    assert!(
        !validator.is_valid(&provider_structured),
        "run_show schema accepts the removed native-only evidence field"
    );
}

#[test]
fn work_view_schema_accepts_typed_writer_observations_and_refuses_removed_shapes() {
    use message_board::{
        CopySource, CurrentWriter, EpochNumber, FeedCursor, FeedPosition, ProjectId, WorkView,
    };

    let validator = advertised_output_validator("view_work");
    let project = ProjectId::generate();
    let head = FeedCursor {
        epoch: EpochNumber::try_from(4).expect("fixture epoch"),
        position: FeedPosition::try_from(17).expect("fixture head"),
    };
    let granted = EpochNumber::try_from(9).expect("actual grant differs from inherited head");
    for (source, writer, expected_writer) in [
        (
            CopySource::Writer {},
            CurrentWriter::Held { epoch: granted },
            serde_json::json!({"kind":"held","epoch":9}),
        ),
        (
            CopySource::ReadCopy {},
            CurrentWriter::Unheld {},
            serde_json::json!({"kind":"unheld"}),
        ),
        (
            CopySource::ReadCopy {},
            CurrentWriter::Held { epoch: granted },
            serde_json::json!({"kind":"held","epoch":9}),
        ),
        (
            CopySource::ReadCopy {},
            CurrentWriter::Unknown {},
            serde_json::json!({"kind":"unknown"}),
        ),
    ] {
        let view = WorkView {
            project: project.clone(),
            as_of: head,
            replicated_through: head,
            workstreams: Vec::new(),
            ready_queue: Vec::new(),
            blocked: Vec::new(),
            open_in_closed_workstreams: Vec::new(),
            current_writer: writer,
            writer_observation: None,
            source,
        };
        let encoded = validated_structured_content(
            &validator,
            "view_work",
            super::super::success_result(&view),
        );
        assert_eq!(encoded["currentWriter"], expected_writer);
        assert_eq!(
            encoded["asOf"],
            serde_json::json!({"epoch":4,"position":17})
        );
        assert_eq!(encoded["replicatedThrough"], encoded["asOf"]);
        assert_eq!(
            encoded["source"],
            match source {
                CopySource::Writer {} => serde_json::json!({"kind":"writer"}),
                CopySource::ReadCopy {} => serde_json::json!({"kind":"readCopy"}),
            }
        );
        assert!(encoded.get("currentEpoch").is_none());
        let mut provisional = encoded.clone();
        provisional["currentWriter"] = serde_json::json!({"kind":"provisional"});
        assert!(
            !validator.is_valid(&provisional),
            "removed provisional writer remains advertised"
        );
        let mut tail_report = encoded.clone();
        tail_report["rejectedTails"] = serde_json::json!([]);
        assert!(
            !validator.is_valid(&tail_report),
            "removed tail bookkeeping remains advertised"
        );

        let mut legacy = encoded.clone();
        legacy
            .as_object_mut()
            .expect("view object")
            .remove("currentWriter");
        legacy["currentEpoch"] = serde_json::json!(9);
        assert!(
            !validator.is_valid(&legacy),
            "old currentEpoch shape remains advertised"
        );
        for removed_source in [
            serde_json::json!("writer"),
            serde_json::json!("readCopy"),
            serde_json::json!({"kind":"superseded","supersededBy":9}),
            serde_json::json!({"kind":"superseded","supersededBy":null}),
        ] {
            let mut invalid = encoded.clone();
            invalid["source"] = removed_source;
            assert!(
                !validator.is_valid(&invalid),
                "removed copy source remains advertised: {invalid}"
            );
        }
    }
}

#[test]
fn cards_and_cross_project_rows_require_their_own_copy_context() {
    use message_board::{
        ContinuationCard, CopySource, EpochNumber, FeedCursor, FeedPosition, HumanId, Identity,
        LeadLoad, LeadObligation, LeadOwed, OwnerView, OwnerWait, ProjectId, RecordedActor,
        SessionRunningState,
    };

    let project = ProjectId::generate();
    let head = FeedCursor {
        epoch: EpochNumber::try_from(4).expect("fixture epoch"),
        position: FeedPosition::try_from(17).expect("fixture head"),
    };
    let since = FeedCursor {
        position: FeedPosition::try_from(3).expect("question position"),
        ..head
    };
    let lead = RecordedActor::declared(Identity::Human {
        human_id: HumanId::try_from("view schema owner".to_owned()).expect("fixture identity"),
    });
    for source in [CopySource::Writer {}, CopySource::ReadCopy {}] {
        let owed = LeadOwed {
            project: project.clone(),
            subject: project.clone().into(),
            obligation: LeadObligation::CloseOut,
            source,
            as_of: head,
            replicated_through: head,
        };
        let owner = OwnerView {
            as_of: "2026-10-19T00:00:00Z"
                .parse()
                .expect("fixture observation time"),
            waiting: vec![OwnerWait {
                project: project.clone(),
                subject: project.clone().into(),
                blocks: Vec::new(),
                since,
                source,
                as_of: head,
                replicated_through: head,
            }],
            unavailable_projects: Vec::new(),
        };
        let load = LeadLoad {
            lead: lead.clone(),
            owed: vec![owed.clone()],
            unavailable_projects: Vec::new(),
        };
        let card = ContinuationCard {
            subject: project.clone().into(),
            plan_revision: None,
            assignment: None,
            pending_handbacks: Vec::new(),
            pending_decisions: Vec::new(),
            next_obligation: Some(owed),
            recent_trail: Vec::new(),
            counts: std::collections::BTreeMap::new(),
            as_of: head,
            replicated_through: head,
            source,
            silence_rule: "fixture silence rule".to_owned(),
            notify_intents: Vec::new(),
            blockers: Vec::new(),
            session_state: SessionRunningState::NotSession {},
        };
        let cases = [
            (
                "view_owner",
                super::super::success_result(&owner),
                "/waiting/0",
            ),
            (
                "view_lead_load",
                super::super::success_result(&load),
                "/owed/0",
            ),
            (
                "view_card",
                super::super::success_result(&card),
                "/nextObligation",
            ),
        ];
        for (tool, result, row_path) in cases {
            let validator = advertised_output_validator(tool);
            let encoded = validated_structured_content(&validator, tool, result);
            let row = encoded.pointer(row_path).expect("actual row payload");
            assert_eq!(
                row["source"],
                serde_json::to_value(source).expect("copy label")
            );
            assert_eq!(row["asOf"], serde_json::json!({"epoch":4,"position":17}));
            assert_eq!(row["replicatedThrough"], row["asOf"]);
            if tool == "view_owner" {
                assert_eq!(row["since"], serde_json::json!({"epoch":4,"position":3}));
            }
            for required in ["source", "asOf", "replicatedThrough"] {
                let mut incomplete = encoded.clone();
                assert!(
                    incomplete
                        .pointer_mut(row_path)
                        .expect("row")
                        .as_object_mut()
                        .expect("row object")
                        .remove(required)
                        .is_some()
                );
                assert!(
                    !validator.is_valid(&incomplete),
                    "{tool} accepts a row missing {required}"
                );
            }
        }
    }
}

#[test]
fn remote_status_history_and_requested_cancel_results_keep_closed_advertised_shapes() {
    use collaboration_protocol::{
        MailReadContext, MessageHistoryEntry, MessageHistoryResult, MessageShowResult, PushOrigin,
        RemoteMessageStatus, RequestedCancelOutcome, RequestedCancelResult,
    };
    let show = advertised_output_validator("router_show");
    let history = advertised_output_validator("message_history");
    let item: collaboration_protocol::MailItemId = FIXTURE_PUSH_ID.to_owned().try_into().unwrap();
    let context = MailReadContext::Direct {
        origin: PushOrigin::OwnerUnverified,
        recipient: serde_json::from_value(fixture_session("codex-local", "recipient")).unwrap(),
    };
    for status in [
        serde_json::json!({"kind":"queued"}),
        serde_json::json!({"kind":"sent","seq":7}),
        serde_json::json!({"kind":"atMachine"}),
        serde_json::json!({"kind":"held"}),
        serde_json::json!({"kind":"delivered","acceptance":"peerMessageWritten"}),
        serde_json::json!({"kind":"uncertain"}),
        serde_json::json!({"kind":"expired"}),
        serde_json::json!({"kind":"cancelled"}),
        serde_json::json!({"kind":"refused","reason":{"kind":"recipientRevoked"}}),
    ] {
        let mail = RemoteMessageStatus {
            item: item.clone(),
            context: context.clone(),
            recorded_at: "2026-10-22T00:00:00Z".parse().unwrap(),
            status: serde_json::from_value(status.clone()).unwrap(),
        };
        let mut shown = validated_structured_content(
            &show,
            "router_show",
            super::super::success_result(&MessageShowResult::Remote {
                mail: Box::new(mail.clone()),
            }),
        );
        assert_eq!(shown["mail"]["status"], status);
        assert!(
            shown.get("record").is_none(),
            "sender custody invented a push"
        );
        shown["mail"]["status"] = serde_json::json!({"kind":"provisional"});
        assert!(
            !show.is_valid(&shown),
            "remote status accepted an unknown state"
        );
        let encoded = validated_structured_content(
            &history,
            "message_history",
            super::super::success_result(&MessageHistoryResult {
                records: vec![MessageHistoryEntry::Remote { mail }],
            }),
        );
        assert_eq!(encoded["records"][0]["mail"]["status"], status);
    }
    for tool in ["message_cancel", "project_notification_cancel"] {
        let validator = advertised_output_validator(tool);
        for outcome in [
            serde_json::json!({"kind":"pending"}),
            serde_json::json!({"kind":"prevented"}),
            serde_json::json!({"kind":"tooLate","at":"delivered"}),
            serde_json::json!({"kind":"tooLate","at":"uncertain"}),
            serde_json::json!({"kind":"refused","reason":{"kind":"mailboxFull"}}),
        ] {
            let result = RequestedCancelResult {
                cancel: item.clone(),
                target: item.clone(),
                outcome: serde_json::from_value::<RequestedCancelOutcome>(outcome.clone()).unwrap(),
            };
            let mut encoded = validated_structured_content(
                &validator,
                tool,
                super::super::success_result(&result),
            );
            assert_eq!(encoded["outcome"], outcome);
            encoded["outcome"] = serde_json::json!({"kind":"done"});
            assert!(
                !validator.is_valid(&encoded),
                "{tool} accepted an unknown control result"
            );
        }
    }
}
