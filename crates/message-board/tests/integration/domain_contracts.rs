use message_board::{
    ActingForIdentity, ActivitySequence, BoardId, CommonDirectory, HumanId, Identity,
    NormalizedOrigin, PostId, PostListScope, PostReferences, PostSelection, PostText, ProjectId,
    ReferenceTarget, ResourceName, ServiceId, SessionEndpointRef, SessionId, SessionRef, ThreadId,
    ThreadPostRequest, WorkspaceId, normalize_git_origin_url,
};
use serde_json::json;

fn id_string(sequence_suffix: u32) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{sequence_suffix:012x}")
}

#[test]
fn inbox_fetch_requires_explicit_top_level_scope_and_supports_latest() {
    let request = serde_json::from_value::<message_board::InboxFetchRequest>(json!({
        "scope": {"kind": "project", "projectId": id_string(1)},
        "protocolVersion":1,"actingFor":null,"initializations":[],
        "reader": {"kind": "human", "humanId": "reader"},
        "readMode": "latest",
        "page": {}
    }));
    assert!(
        request.is_ok(),
        "scoped latest request must decode: {request:?}"
    );
    let missing_scope = serde_json::from_value::<message_board::InboxFetchRequest>(json!({
        "protocolVersion":1,"actingFor":null,"initializations":[],
        "reader": {"kind": "human", "humanId": "reader"},
        "readMode": "unread",
        "page": {}
    }));
    assert!(missing_scope.is_err());
}

#[test]
fn search_queries_are_trimmed_nonempty_and_utf8_byte_bounded() {
    let query = message_board::SearchQuery::try_from(" \tneedle\n ".to_owned()).unwrap();
    assert_eq!(query.as_str(), "needle");
    assert!(message_board::SearchQuery::try_from(" \t\n ".to_owned()).is_err());
    assert!(message_board::SearchQuery::try_from("n".repeat(256)).is_ok());
    assert!(message_board::SearchQuery::try_from("n".repeat(257)).is_err());
    assert!(message_board::SearchQuery::try_from("é".repeat(128)).is_ok());
    assert!(message_board::SearchQuery::try_from("é".repeat(129)).is_err());
}

#[test]
fn resource_ids_accept_only_canonical_uuid_v7() {
    let workspace_id = WorkspaceId::try_from(id_string(1)).expect("fixture is UUIDv7");
    assert_eq!(workspace_id.as_str(), id_string(1));

    assert!(WorkspaceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned()).is_err());
    assert!(WorkspaceId::try_from(id_string(2).to_uppercase()).is_err());
}

#[test]
fn metadata_names_are_trimmed_and_byte_bounded() {
    let name = ResourceName::try_from("  Release coordination  ".to_owned())
        .expect("trimmed name is valid");
    assert_eq!(name.as_str(), "Release coordination");
    assert!(ResourceName::try_from("   ".to_owned()).is_err());
    assert!(ResourceName::try_from("é".repeat(129)).is_err());
}

#[test]
fn session_identity_retains_the_complete_endpoint() {
    let identity = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())
                    .expect("canonical service ID"),
                endpoint_id: "local-codex".to_owned().try_into().expect("endpoint ID"),
            },
            session_id: SessionId::try_from("thread-42".to_owned()).expect("session ID"),
        },
    };

    assert_eq!(
        serde_json::to_value(&identity).expect("serialize identity"),
        json!({
            "kind": "session",
            "session": {
                "endpoint": {
                    "serviceId": "550e8400-e29b-41d4-a716-446655440000",
                    "endpointId": "local-codex"
                },
                "sessionId": "thread-42"
            }
        })
    );
    assert_eq!(identity.short_form(), "local:thread-42");
    assert!(
        serde_json::from_value::<Identity>(json!({
            "kind": "human", "humanId": "owner", "unexpected": true
        }))
        .is_err()
    );
    assert!(HumanId::try_from("owner\0shadow".to_owned()).is_err());
}

#[test]
fn acting_for_contract_can_only_represent_a_human() {
    let acting_for = ActingForIdentity::Human {
        human_id: HumanId::try_from("owner".to_owned()).expect("human ID"),
    };
    assert_eq!(
        serde_json::to_value(acting_for).expect("serialize acting-for"),
        json!({
            "kind": "human", "humanId": "owner"
        })
    );
    assert!(serde_json::from_value::<ActingForIdentity>(json!({
        "kind": "session",
        "session": {"endpoint": {"serviceId": "550e8400-e29b-41d4-a716-446655440000", "endpointId": "local"}, "sessionId": "x"}
    })).is_err());
}

#[test]
fn post_contract_uses_closed_camel_case_variants() {
    let request = ThreadPostRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: message_board::ProtocolVersion::CURRENT,
        post_id: PostId::try_from(id_string(4)).expect("message ID"),
        placement: message_board::PostTarget::Thread {
            thread: ThreadId::try_from(id_string(3)).expect("root ID").into(),
        },
        actor: Identity::Human {
            human_id: HumanId::try_from("reviewer".to_owned()).expect("human ID"),
        },
        acting_for: None,
        text: PostText::try_from("Please inspect this edge.".to_owned()).expect("message text"),
        addressees: Vec::new(),
        references: PostReferences::try_from(vec![ReferenceTarget::Post {
            post_id: PostId::try_from(id_string(2)).expect("target ID"),
        }])
        .expect("references"),
    };
    let encoded = serde_json::to_value(request).expect("serialize request");
    assert_eq!(encoded["placement"]["kind"], "thread");
    assert_eq!(encoded["placement"]["thread"]["thread"], id_string(3));
    assert_eq!(encoded["references"][0]["kind"], "post");
}

#[test]
fn duplicate_and_excess_references_are_rejected() {
    let target = ReferenceTarget::Thread {
        thread_id: ThreadId::try_from(id_string(1)).expect("root ID"),
    };
    assert!(PostReferences::try_from(vec![target.clone(), target]).is_err());

    let references: Vec<ReferenceTarget> = (1..=65)
        .map(|suffix| ReferenceTarget::Post {
            post_id: PostId::try_from(id_string(suffix)).expect("message ID"),
        })
        .collect();
    assert!(PostReferences::try_from(references).is_err());
}

#[test]
fn history_variants_preserve_scope_and_validate_range_order() {
    let project_position = |position| message_board::ReadPosition::Project {
        cursor: message_board::FeedCursor {
            epoch: message_board::EpochNumber::try_from(1).unwrap(),
            position: message_board::FeedPosition::try_from(position).unwrap(),
        },
    };
    let scope = PostListScope::Workspace {
        workspace_id: WorkspaceId::try_from(id_string(1)).expect("workspace ID"),
    };
    let selection = PostSelection::AfterPosition {
        after: project_position(40),
    };
    assert_eq!(
        serde_json::to_value(scope).expect("serialize scope")["kind"],
        "workspace"
    );
    assert_eq!(
        serde_json::to_value(selection).expect("serialize selection"),
        json!({
            "kind": "afterPosition", "after":{"kind":"project","cursor":{"epoch":1,"position":40}}
        })
    );
    assert!(
        PostSelection::Range {
            from: project_position(41),
            to: project_position(40),
        }
        .validate()
        .is_err()
    );
    assert!(
        serde_json::from_value::<PostSelection>(json!({
            "kind": "range",
            "from":{"kind":"project","cursor":{"epoch":1,"position":41}},
            "to":{"kind":"project","cursor":{"epoch":1,"position":40}}
        }))
        .is_err()
    );
}

#[test]
fn activity_positions_fit_the_sqlite_signed_integer_domain() {
    assert_eq!(ActivitySequence::ZERO.get(), 0);
    assert_eq!(
        ActivitySequence::try_from(i64::MAX as u64)
            .expect("maximum SQLite integer")
            .get(),
        i64::MAX as u64
    );
    assert!(ActivitySequence::try_from(i64::MAX as u64 + 1).is_err());
    assert!(serde_json::from_value::<ActivitySequence>(json!(i64::MAX as u64 + 1)).is_err());
}

#[test]
fn participant_role_note_and_closed_state_are_validated_in_rust() {
    use message_board::{
        Participant, ParticipantClosedReason, ParticipantNote, ParticipantProps, ParticipantRole,
        RecordedParticipantRole,
    };
    assert!(serde_json::from_value::<ParticipantRole>(json!("orchestrator")).is_ok());
    assert!(serde_json::from_value::<ParticipantRole>(json!("driver")).is_err());
    let note = ParticipantNote::try_from("  reviewing storage  ".to_owned()).unwrap();
    assert_eq!(note.as_str(), "reviewing storage");
    assert!(ParticipantNote::try_from("  ".to_owned()).is_err());
    assert!(ParticipantNote::try_from("é".repeat(8_192)).is_ok());
    assert!(ParticipantNote::try_from("é".repeat(8_193)).is_err());

    let identity = Identity::Human {
        human_id: HumanId::try_from("reviewer".to_owned()).unwrap(),
    };
    let replacement = Identity::Human {
        human_id: HumanId::try_from("successor".to_owned()).unwrap(),
    };
    let joined = message_board::FeedCursor {
        epoch: message_board::EpochNumber::try_from(1).unwrap(),
        position: message_board::FeedPosition::try_from(10).unwrap(),
    };
    let closed = message_board::FeedCursor {
        position: message_board::FeedPosition::try_from(12).unwrap(),
        ..joined
    };
    assert!(
        Participant::new(ParticipantProps {
            identity: identity.clone(),
            role: RecordedParticipantRole::Recorded(ParticipantRole::Reviewer),
            note: Some(note),
            joined_at_position: joined,
            last_seen_position: joined,
            closed_at_position: None,
            closed_reason: None,
            replaced_by: None,
        })
        .is_ok()
    );
    for reason in [
        ParticipantClosedReason::Left,
        ParticipantClosedReason::Rejoined,
        ParticipantClosedReason::Promoted,
    ] {
        let props = ParticipantProps {
            identity: identity.clone(),
            role: RecordedParticipantRole::Recorded(ParticipantRole::Reviewer),
            note: None,
            joined_at_position: joined,
            last_seen_position: closed,
            closed_at_position: Some(closed),
            closed_reason: Some(reason),
            replaced_by: None,
        };
        let interval = Participant::new(props.clone()).expect("closed interval");
        assert_eq!(
            serde_json::from_value::<Participant>(serde_json::to_value(&interval).unwrap())
                .unwrap(),
            interval
        );
        assert!(
            Participant::new(ParticipantProps {
                replaced_by: Some(replacement.clone()),
                ..props.clone()
            })
            .is_err(),
            "non-replacement closure accepted a replacement actor"
        );
        assert!(
            Participant::new(ParticipantProps {
                last_seen_position: joined,
                ..props
            })
            .is_err(),
            "closure and last-seen positions differed"
        );
    }
    assert!(
        Participant::new(ParticipantProps {
            identity: identity.clone(),
            role: RecordedParticipantRole::Recorded(ParticipantRole::Reviewer),
            note: None,
            joined_at_position: joined,
            last_seen_position: closed,
            closed_at_position: Some(closed),
            closed_reason: Some(ParticipantClosedReason::Replaced),
            replaced_by: Some(replacement.clone()),
        })
        .is_err(),
        "only an Orchestrator can be closed as replaced"
    );
    assert!(
        Participant::new(ParticipantProps {
            identity: identity.clone(),
            role: RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator),
            note: None,
            joined_at_position: joined,
            last_seen_position: closed,
            closed_at_position: Some(closed),
            closed_reason: Some(ParticipantClosedReason::Replaced),
            replaced_by: Some(identity),
        })
        .is_err(),
        "self replacement is not a valid stored state"
    );
    assert!(
        Participant::new(ParticipantProps {
            identity: replacement,
            role: RecordedParticipantRole::Recorded(ParticipantRole::Participant),
            note: None,
            joined_at_position: closed,
            last_seen_position: joined,
            closed_at_position: None,
            closed_reason: None,
            replaced_by: None,
        })
        .is_err(),
        "last-seen cannot precede Join"
    );
}

#[test]
fn participant_deserialization_rejects_unknown_closed_sets_and_cross_fields() {
    let base = json!({
        "identity": {"kind": "human", "humanId": "owner"},
        "role": {"kind":"recorded","role":"orchestrator"},
        "note": null,
        "joinedAtPosition": {"epoch":1,"position":1},
        "lastSeenPosition": {"epoch":1,"position":2},
        "closedAtPosition": {"epoch":1,"position":2},
        "closedReason": "replaced",
        "replacedBy": {"kind": "human", "humanId": "successor"}
    });
    assert!(serde_json::from_value::<message_board::Participant>(base.clone()).is_ok());
    let mut unknown_reason = base.clone();
    unknown_reason["closedReason"] = json!("expired");
    assert!(serde_json::from_value::<message_board::Participant>(unknown_reason).is_err());
    let mut missing_replacement = base;
    missing_replacement["replacedBy"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<message_board::Participant>(missing_replacement).is_err());
}

#[test]
fn repository_origins_strip_credentials_and_preserve_path_case() {
    assert_eq!(
        normalize_git_origin_url("https://token@GitHub.COM/ShravanSunder/MyRepo.git?x=1#fragment"),
        Some("github.com/ShravanSunder/MyRepo".to_owned())
    );
    assert_eq!(
        normalize_git_origin_url("git@github.com:ShravanSunder/MyRepo.git"),
        Some("github.com/ShravanSunder/MyRepo".to_owned())
    );
    assert!(NormalizedOrigin::try_from("GitHub.com/owner/repo".to_owned()).is_err());
}

#[test]
fn repository_locators_enforce_and_publish_the_utf8_byte_bound() {
    assert!(NormalizedOrigin::try_from(format!("github.com/{}", "a".repeat(4_085))).is_ok());
    assert!(NormalizedOrigin::try_from(format!("github.com/{}", "a".repeat(4_086))).is_err());
    assert!(CommonDirectory::try_from(format!("/{}", "a".repeat(4_095))).is_ok());
    assert!(CommonDirectory::try_from(format!("/{}", "a".repeat(4_096))).is_err());
    assert!(CommonDirectory::try_from(format!("/{}", "é".repeat(2_048))).is_err());

    let origin_schema = schemars::schema_for!(NormalizedOrigin);
    let directory_schema = schemars::schema_for!(CommonDirectory);
    assert_eq!(
        origin_schema.get("maxLength"),
        Some(&serde_json::json!(4096))
    );
    assert_eq!(
        directory_schema.get("maxLength"),
        Some(&serde_json::json!(4096))
    );
}

#[test]
fn distinct_resource_id_types_do_not_interchange() {
    let _board_id = BoardId::try_from(id_string(1)).expect("board ID");
    let _project_id = ProjectId::try_from(id_string(2)).expect("project ID");
    let _workspace_id = WorkspaceId::try_from(id_string(3)).expect("workspace ID");
}

#[test]
fn every_public_operation_has_a_typed_schema_contract() {
    fn assert_contract<T>()
    where
        T: serde::Serialize + serde::de::DeserializeOwned + schemars::JsonSchema,
    {
    }
    macro_rules! contracts {
        ($($contract:ty),+ $(,)?) => { $(assert_contract::<$contract>();)+ };
    }
    use message_board::*;
    contracts!(
        RequestResult,
        WorkspaceCreateRequest,
        WorkspaceUpdateRequest,
        WorkspaceShowRequest,
        WorkspaceShowResult,
        WorkspaceListRequest,
        WorkspaceListResult,
        RepositoryAttachRequest,
        RepositoryDetachRequest,
        RepositoryListRequest,
        RepositoryListResult,
        BoardCreateRequest,
        BoardUpdateRequest,
        BoardShowRequest,
        BoardShowResult,
        BoardListRequest,
        BoardListResult,
        BoardArchiveRequest,
        ProjectCreateRequest,
        ProjectUpdateRequest,
        ProjectListRequest,
        ProjectListResult,
        ThreadPostRequest,
        PostShowRequest,
        PostShowResult,
        PostListRequest,
        PostListResult,
        ThreadShowRequest,
        ThreadShowResult,
        ThreadResolveRequest,
        ThreadUnresolveRequest,
        ThreadWatchRequest,
        ThreadUnwatchRequest,
        ThreadListRequest,
        ThreadListResult,
        ThreadCreateRequest,
        ThreadJoinRequest,
        ThreadLeaveRequest,
        ThreadParticipantListRequest,
        ThreadParticipantListResult,
        InboxFetchRequest,
        InboxFetchResult,
        InboxAcknowledgeRequest,
        InboxWorkspacesRequest,
        InboxWorkspacesResult,
    );
}

#[test]
fn actionable_failure_factories_preserve_closed_details() {
    use message_board::{
        BoardError, BoardErrorDetails, BoardFailureKind, BoardNextAction, PostText, ProjectId,
    };
    let failure = BoardError::top_level_message_cooldown(7);
    assert_eq!(failure.kind, BoardFailureKind::TopLevelPostCooldown);
    assert_eq!(failure.next_action, BoardNextAction::PostThreadPost);
    assert_eq!(
        failure.details,
        BoardErrorDetails::Cooldown {
            retry_after_seconds: 7
        }
    );
    assert!(failure.message.contains("Wait 7 seconds"));
    assert_eq!(
        BoardError::thread_resolved().message,
        "This thread is resolved. Mark it unresolved before adding a thread message."
    );
    let project_id = ProjectId::try_from(id_string(92)).expect("project ID");
    let text = PostText::try_from("reuse this submitted root text".to_owned()).expect("text");
    let project_refusal = BoardError::session_project_post(
        project_id,
        Identity::Human {
            human_id: HumanId::try_from("project-author".to_owned()).expect("human ID"),
        },
        text.clone(),
    );
    let BoardErrorDetails::ThreadCreateRefusal { refusal } = project_refusal.details else {
        panic!("expected Thread Create refusal details");
    };
    assert_eq!(refusal.text, text);
}
