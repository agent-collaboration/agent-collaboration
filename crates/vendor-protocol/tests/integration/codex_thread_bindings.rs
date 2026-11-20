use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use vendor_protocol::codex::{
    ApprovalsReviewer, AskForApproval, CodexErrorInfo, CodexRequest, MultiAgentMode,
    MultiAgentModeTag, SandboxPolicy, StringAlternative, ThreadItem, ThreadReadParams,
    ThreadReadResponse, ThreadResumeParams, ThreadResumeResponse, ThreadUnreadPosition,
    TurnItemsView,
};

fn thread_fixture() -> Value {
    json!({
        "cliVersion": "0.166.0",
        "createdAt": 0,
        "cwd": "/fixture",
        "ephemeral": false,
        "id": "fixture-thread",
        "modelProvider": "fixture-provider",
        "preview": "",
        "projectId": null,
        "sessionId": "fixture-session",
        "source": "cli",
        "status": {"type":"idle"},
        "turns": [],
        "updatedAt": 1
    })
}

fn resume_fixture() -> Value {
    json!({
        "approvalPolicy": "on-request",
        "approvalsReviewer": "auto_review",
        "cwd": "/fixture",
        "model": "fixture-model",
        "modelProvider": "fixture-provider",
        "sandbox": {"type":"workspaceWrite"},
        "thread": thread_fixture()
    })
}

#[expect(
    clippy::expect_used,
    reason = "This helper only augments literal object fixtures."
)]
fn with_field(mut object: Value, field: &str, value: Value) -> Value {
    object
        .as_object_mut()
        .expect("object fixture")
        .insert(field.to_owned(), value);
    object
}

#[expect(
    clippy::expect_used,
    reason = "Valid literal wire fixtures must round-trip."
)]
fn round_trip<Payload: DeserializeOwned + serde::Serialize>(wire: &Value) -> Payload {
    let decoded: Payload = serde_json::from_str(&wire.to_string()).expect("valid literal wire");
    assert_eq!(
        serde_json::to_value(&decoded).expect("serialized wire"),
        *wire
    );
    decoded
}

#[test]
fn method_associations_are_selected_by_the_request_types() {
    fn response_for<Request: CodexRequest>(body: Value) -> serde_json::Result<Request::Response> {
        serde_json::from_value(body)
    }

    let read: ThreadReadResponse =
        response_for::<ThreadReadParams>(json!({"thread": thread_fixture()})).expect("read reply");
    let resume: ThreadResumeResponse =
        response_for::<ThreadResumeParams>(resume_fixture()).expect("resume reply");

    assert_eq!(ThreadReadParams::METHOD, "thread/read");
    assert_eq!(ThreadResumeParams::METHOD, "thread/resume");
    assert_eq!(read.thread, resume.thread);
    assert!(matches!(resume.sandbox, SandboxPolicy::WorkspaceWrite(_)));
}

#[test]
fn resume_request_preserves_omitted_null_and_set_settings() {
    let base = json!({"threadId":"fixture-thread"});
    let mut explicit_null = base.clone();
    for field in [
        "approvalPolicy",
        "approvalsReviewer",
        "sandbox",
        "permissions",
        "runtimeWorkspaceRoots",
    ] {
        explicit_null = with_field(explicit_null, field, Value::Null);
    }
    let supplied = json!({
        "threadId":"fixture-thread",
        "approvalPolicy":{"granular":{"mcp_elicitations":false,"rules":true,"sandbox_approval":false}},
        "approvalsReviewer":"guardian_subagent",
        "sandbox":"workspace-write",
        "permissions":"fixture-profile",
        "runtimeWorkspaceRoots":["/fixture"],
        "config":{"vendor-extension":{"future":[true,null,12]}},
        "excludeTurns":false
    });

    let omitted: ThreadResumeParams = round_trip(&base);
    let null: ThreadResumeParams = round_trip(&explicit_null);
    let typed: ThreadResumeParams = round_trip(&supplied);

    assert_eq!(omitted.approval_policy, None);
    assert_eq!(omitted.runtime_workspace_roots, None);
    assert_eq!(null.approval_policy, Some(None));
    assert_eq!(null.approvals_reviewer, Some(None));
    assert_eq!(null.sandbox, Some(None));
    assert_eq!(null.permissions, Some(None));
    assert_eq!(null.runtime_workspace_roots, Some(None));
    assert!(matches!(
        typed.approval_policy,
        Some(Some(AskForApproval::Granular(_)))
    ));
    assert_eq!(
        typed.approvals_reviewer,
        Some(Some(ApprovalsReviewer::GuardianSubagent))
    );
    assert_eq!(
        typed.runtime_workspace_roots,
        Some(Some(vec!["/fixture".into()]))
    );

    for invalid in [
        json!({}),
        json!({"threadId":"fixture-thread","sandbox":{"workspace-write":null}}),
        json!({"threadId":"fixture-thread","permissions":3}),
        json!({"threadId":"fixture-thread","config":[]}),
        json!({"threadId":"fixture-thread","runtimeWorkspaceRoots":[false]}),
        json!({"threadId":"fixture-thread","approvalPolicy":{"granular":{"rules":true}}}),
        json!({"threadId":"fixture-thread","approvalPolicy":{
            "granular":{"mcp_elicitations":false,"rules":true,"sandbox_approval":false},
            "unrecognized":true
        }}),
    ] {
        assert!(
            serde_json::from_value::<ThreadResumeParams>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn reply_settings_live_only_on_resume_and_required_nullable_thread_keys_stay_required() {
    let base_resume = resume_fixture();
    let omitted: ThreadResumeResponse = round_trip(&base_resume);
    assert_eq!(omitted.active_permission_profile, None);
    assert_eq!(omitted.runtime_workspace_roots, None);
    assert!(omitted.runtime_workspace_roots().is_empty());
    assert_eq!(
        omitted.multi_agent_mode(),
        MultiAgentMode::Known(MultiAgentModeTag::ExplicitRequestOnly)
    );

    let explicit_null = with_field(base_resume.clone(), "activePermissionProfile", Value::Null);
    let null: ThreadResumeResponse = round_trip(&explicit_null);
    assert_eq!(null.active_permission_profile, Some(None));

    let with_profile = with_field(
        with_field(
            base_resume.clone(),
            "activePermissionProfile",
            json!({"id":"fixture-profile","extends":null}),
        ),
        "runtimeWorkspaceRoots",
        json!(["/fixture"]),
    );
    let populated: ThreadResumeResponse = round_trip(&with_profile);
    assert_eq!(
        populated.active_permission_profile().expect("profile").id,
        "fixture-profile"
    );
    assert_eq!(populated.runtime_workspace_roots(), ["/fixture"]);

    for field in ["approvalPolicy", "approvalsReviewer", "sandbox"] {
        let mut missing = base_resume.clone();
        missing.as_object_mut().expect("reply object").remove(field);
        assert!(
            serde_json::from_value::<ThreadResumeResponse>(missing).is_err(),
            "{field}"
        );
        let null = with_field(base_resume.clone(), field, Value::Null);
        assert!(
            serde_json::from_value::<ThreadResumeResponse>(null).is_err(),
            "{field}"
        );
    }
    assert!(
        serde_json::from_value::<ThreadResumeResponse>(with_field(
            base_resume,
            "runtimeWorkspaceRoots",
            Value::Null
        ))
        .is_err()
    );

    let base_thread = thread_fixture();
    let mut additive_thread = base_thread.clone();
    let mut read = json!({"thread": base_thread});
    for (field, value) in [
        ("approvalPolicy", json!("never")),
        ("approvalsReviewer", json!("user")),
        ("sandbox", json!({"type":"readOnly"})),
        ("permissions", json!("fixture-profile")),
        ("activePermissionProfile", json!({"id":"fixture-profile"})),
        ("runtimeWorkspaceRoots", json!(["/fixture"])),
    ] {
        additive_thread = with_field(additive_thread, field, value.clone());
        read = with_field(read, field, value);
    }
    read = with_field(read, "thread", additive_thread);
    let decoded: ThreadReadResponse =
        serde_json::from_value(read).expect("read with additive keys");
    assert_eq!(
        serde_json::to_value(decoded).expect("encoded read"),
        json!({"thread":base_thread})
    );

    let mut missing_project = thread_fixture();
    missing_project
        .as_object_mut()
        .expect("thread object")
        .remove("projectId");
    assert!(
        serde_json::from_value::<ThreadReadResponse>(json!({"thread":missing_project})).is_err()
    );
}

#[test]
fn read_state_and_thread_tags_preserve_wire_keys_presence_and_object_shape() {
    let base = json!({"thread":thread_fixture()});
    let missing: ThreadReadResponse = round_trip(&base);
    assert_eq!(missing.read_state, None);
    let null: ThreadReadResponse = round_trip(&with_field(base.clone(), "readState", Value::Null));
    assert_eq!(null.read_state, Some(None));
    let state: ThreadReadResponse = round_trip(&with_field(
        base.clone(),
        "readState",
        json!({
            "revision":"fixture-revision",
            "firstUnread":{"type":"turn","turnId":"fixture-turn"}
        }),
    ));
    let state = state.read_state.flatten().expect("read state");
    assert!(matches!(
        state.first_unread,
        Some(Some(ThreadUnreadPosition::Turn(_)))
    ));

    let declared: ThreadReadParams =
        round_trip(&json!({"threadId":"fixture-thread","includeTurns":false}));
    assert_eq!(declared.include_turns, Some(false));
    for invalid in [
        json!({"readState":{"revision":"revision"}}),
        with_field(
            base.clone(),
            "readState",
            json!({"revision":"revision","firstUnread":{"type":"turn"}}),
        ),
        with_field(
            base,
            "readState",
            json!({"revision":"revision","firstUnread":{"type":{"threadStart":null}}}),
        ),
        json!({"thread":with_field(thread_fixture(),"status",json!(["idle"]))}),
        json!({"thread":with_field(thread_fixture(),"status",json!({"type":"active","activeFlags":[{"waitingOnApproval":null}]}))}),
    ] {
        assert!(
            serde_json::from_value::<ThreadReadResponse>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
    assert!(
        serde_json::from_value::<ThreadReadParams>(
            json!({"threadId":"fixture-thread","includeTurns":null})
        )
        .is_err()
    );
}

fn read_with_items(items: Value) -> Value {
    let turn = json!({"id":"turn-example","status":"completed","items":items});
    json!({"thread":with_field(thread_fixture(),"turns",json!([turn]))})
}

#[test]
fn branch_local_image_alternatives_keep_valid_and_unvalidated_wire_values_separate() {
    for content in [
        json!({"type":"image","url":"fixture.jpg","detail":"high"}),
        json!({"type":"image","fileId":"fixture-file"}),
        json!({"type":"image","url":"fixture.jpg","fileId":"fixture-file"}),
        json!({"type":"image","url":{"invalid":true},"fileId":"fixture-file"}),
    ] {
        let user_item =
            json!({"type":"userMessage","id":"item-example","content":[content],"startedAtMs":0});
        round_trip::<ThreadReadResponse>(&read_with_items(json!([user_item])));
    }
    for content in [
        json!({"type":"image"}),
        json!({"type":"image","url":{"invalid":true},"fileId":null}),
        json!({"type":"image","fileId":"fixture-file","detail":{"high":null}}),
    ] {
        let user_item = json!({"type":"userMessage","id":"item-example","content":[content]});
        assert!(
            serde_json::from_value::<ThreadReadResponse>(read_with_items(json!([user_item])))
                .is_err()
        );
    }

    let generated = json!({
        "type":"imageGeneration","id":"generated","status":"completed","fileId":"fixture-file","result":""
    });
    let wire = read_with_items(json!([generated]));
    let mut decoded: ThreadReadResponse = round_trip(&wire);
    let turn = decoded.thread.turns.first_mut().expect("turn");
    assert_eq!(turn.items_view(), TurnItemsView::Full);
    let Some(ThreadItem::ImageGeneration(image)) = turn.items.first_mut() else {
        panic!("expected typed image generation");
    };
    assert_eq!(
        image.file_id,
        Some(StringAlternative::Valid("fixture-file".into()))
    );
    assert_eq!(image.result, Some(StringAlternative::Valid(String::new())));
    image.file_id = None;
    image.result = Some(StringAlternative::Malformed(Value::Null));
    assert!(
        serde_json::to_value(&decoded).is_err(),
        "constructed payload needs a valid alternative"
    );

    round_trip::<ThreadResumeParams>(&json!({"threadId":"fixture-thread","history":[
        {"type":"message","role":"user","content":[{"type":"input_image","image_url":"fixture.jpg","file_id":7}]},
        {"type":"function_call_output","output":[{"type":"input_image","image_url":null,"file_id":"fixture-file"}]},
        {"type":"function_call_output","output":"text"}
    ]}));
}

#[test]
fn explicitly_open_error_detail_preserves_terminal_turn_and_validation_state() {
    let cases = [
        json!({"httpConnectionFailed":{"httpStatusCode":65535}}),
        json!("futureFixtureFailure"),
        json!({"futureFixtureFailure":{"receipt":[1,null]}}),
        json!({"httpConnectionFailed":{"httpStatusCode":"malformed"}}),
        json!({"httpConnectionFailed":{"httpStatusCode":65536}}),
    ];

    for (index, detail) in cases.into_iter().enumerate() {
        let turn = json!({
            "id":"terminal-turn","status":"failed","items":[],
            "error":{"message":"fixture failure","codexErrorInfo":detail}
        });
        let wire = json!({"thread":with_field(thread_fixture(),"turns",json!([turn]))});
        let decoded: ThreadReadResponse = round_trip(&wire);
        let error = decoded
            .thread
            .turns
            .first()
            .expect("terminal turn")
            .error
            .as_ref()
            .and_then(Option::as_ref)
            .expect("turn error");
        let detail = error
            .codex_error_info
            .as_ref()
            .and_then(Option::as_ref)
            .expect("error detail");
        assert!(matches!(
            (index, detail),
            (0, CodexErrorInfo::HttpConnectionFailed(_))
                | (1, CodexErrorInfo::UnknownString(_))
                | (2, CodexErrorInfo::UnknownObject(_))
                | (3 | 4, CodexErrorInfo::MalformedKnownObject(_))
        ));
    }
    let turn = json!({
        "id":"terminal-turn","status":"failed","items":[],
        "error":{"message":"fixture failure","codexErrorInfo":["not a supported wire kind"]}
    });
    assert!(
        serde_json::from_value::<ThreadReadResponse>(json!({
            "thread":with_field(thread_fixture(),"turns",json!([turn]))
        }))
        .is_err()
    );
}

#[test]
fn typed_maps_and_numeric_bounds_preserve_only_the_schema_declared_open_subtrees() {
    let history_item = json!({
        "type":"local_shell_call","status":"completed",
        "action":{"type":"exec","command":["fixture"],"env":{"KEY":"literal"},"timeout_ms":u64::MAX}
    });
    round_trip::<ThreadResumeParams>(
        &json!({"threadId":"fixture-thread","history":[history_item]}),
    );
    let bad_map = with_field(
        history_item.clone(),
        "action",
        json!({
            "type":"exec","command":["fixture"],"env":{"KEY":false}
        }),
    );
    let bad_bound = with_field(
        history_item,
        "action",
        json!({
            "type":"exec","command":["fixture"],"timeout_ms":-1
        }),
    );
    for invalid in [bad_map, bad_bound] {
        assert!(
            serde_json::from_value::<ThreadResumeParams>(
                json!({"threadId":"fixture-thread","history":[invalid]})
            )
            .is_err()
        );
    }
    let item = json!({
        "type":"mcpToolCall","id":"fixture-tool","server":"fixture","tool":"fixture","status":"completed",
        "arguments":null,
        "result":{"content":[{},true],"structuredContent":null,"_meta":{"future":"value"}}
    });
    round_trip::<ThreadReadResponse>(&read_with_items(json!([item])));
    let text_item = json!({
        "type":"userMessage","id":"fixture-text",
        "content":[{"type":"text","text":"hi","text_elements":[{"byteRange":{"start":0,"end":2}}]}]
    });
    round_trip::<ThreadReadResponse>(&read_with_items(json!([text_item])));

    let source = |depth: i64| {
        json!({"subAgent":{"thread_spawn":{
            "depth": depth, "parent_thread_id":"parent-fixture"
        }}})
    };
    let valid_thread = with_field(thread_fixture(), "source", source(i64::from(i32::MAX)));
    let valid_thread = with_field(valid_thread, "extra", json!({"future":[true,null]}));
    round_trip::<ThreadReadResponse>(&json!({"thread":valid_thread}));

    let out_of_range = with_field(thread_fixture(), "source", source(i64::from(i32::MAX) + 1));
    let invalid_extra = with_field(thread_fixture(), "extra", json!([]));
    for invalid in [out_of_range, invalid_extra] {
        assert!(serde_json::from_value::<ThreadReadResponse>(json!({"thread":invalid})).is_err());
    }
}

#[test]
fn thread_methods_reject_cross_assigned_requests_and_responses_at_compile_time() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/integration/codex_thread_bindings/cross_method.rs");
}
