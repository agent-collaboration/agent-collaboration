use super::{ClaudeAcpBehavior, ClaudeSkillSource};
use acp_client_runtime::{
    CompactionTrigger, PreparedCompact, ProviderBehavior, ProviderCommand, ProviderError,
    ProviderUpdate,
};
use serde_json::json;
use session_event_model::{
    PromptContent, ProviderSubmission, SelectedSkill, SkillInvocationName, SkillSourceError,
    SkillView, ToolCallStatus,
};

#[test]
fn initialization_authentication_retains_claudes_existing_launch_context() {
    for source in [
        ClaudeSkillSource::Annotated,
        ClaudeSkillSource::Unavailable(SkillSourceError::UnestablishedClassification),
    ] {
        assert_eq!(
            ClaudeAcpBehavior::new(source).initialization_authentication(),
            acp_client_runtime::ProviderInitializationAuthentication::NotRequested
        );
    }
}

#[test]
fn claude_splits_at_last_selected_token_and_puts_command_after_images() {
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated);
    let image = PromptContent::image("image/png".into(), "aGVsbG8=".into(), None).expect("image");
    let content = vec![
        PromptContent::text("前置 $review earlier\n$review\targs  ".into()).expect("text"),
        image.clone(),
    ];
    let selected = vec![SelectedSkill {
        invocation_name: SkillInvocationName::new("review").expect("name"),
    }];
    assert_eq!(
        behavior
            .prepare(ProviderSubmission { content, selected })
            .expect("prepared"),
        vec![
            PromptContent::text("前置 $review earlier\n".into()).expect("prefix"),
            image,
            PromptContent::text("/review\targs  ".into()).expect("final command")
        ]
    );
    assert_eq!(
        behavior.compact().expect("compact"),
        PreparedCompact::Prompt(vec![
            PromptContent::text("/compact".into()).expect("command")
        ])
    );
}

#[test]
fn claude_annotated_empty_differs_from_pending_and_failure() {
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated);
    assert_eq!(
        behavior
            .observe(ProviderUpdate::SessionAdmitted)
            .expect("admission")
            .skill_view,
        Some(SkillView::Unknown)
    );
    let command = ProviderCommand {
        invocation_name: "plugin:review".into(),
        description: "Review changes".into(),
        input_hint: None,
        metadata: Some(json!({"codexRouter":{"kind":"skill"}})),
    };
    let facts = behavior
        .observe(ProviderUpdate::Commands {
            commands: vec![command],
            metadata: Some(json!({"codexRouter":{"skillStatus":"available"}})),
        })
        .expect("metadata");
    let Some(SkillView::Available { skills, errors }) = facts.skill_view else {
        panic!("available");
    };
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].invocation_name.as_str(), "plugin:review");
    assert!(errors.is_empty());
    let unavailable = ClaudeAcpBehavior::new(ClaudeSkillSource::Unavailable(
        SkillSourceError::UnqualifiedLaunch,
    ));
    assert_eq!(
        unavailable
            .observe(ProviderUpdate::SessionAdmitted)
            .expect("source")
            .skill_view,
        Some(SkillView::Unavailable(SkillSourceError::UnqualifiedLaunch))
    );
    assert_eq!(unavailable.compact(), Err(ProviderError::Unsupported));
}

#[test]
fn claude_compaction_marker_uses_version_trigger_tool_id_and_status() {
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated);
    for (metadata, trigger) in [
        (
            json!({"contextCompaction":{"version":1}}),
            CompactionTrigger::Unlabelled,
        ),
        (
            json!({"contextCompaction":{"version":1,"trigger":"manual"}}),
            CompactionTrigger::Manual,
        ),
        (
            json!({"contextCompaction":{"version":1,"trigger":"automatic"}}),
            CompactionTrigger::Automatic,
        ),
    ] {
        let hint = behavior
            .observe(ProviderUpdate::ToolStatus {
                tool_id: "compact-1".into(),
                status: ToolCallStatus::Completed,
                metadata: Some(metadata),
            })
            .expect("decoded marker")
            .compaction
            .expect("hint");
        assert_eq!(
            (hint.tool_id.as_str(), hint.status, hint.trigger),
            ("compact-1", ToolCallStatus::Completed, trigger)
        );
    }
    assert!(
        behavior
            .observe(ProviderUpdate::ToolStatus {
                tool_id: "compact-1".into(),
                status: ToolCallStatus::Completed,
                metadata: Some(json!({"contextCompaction":{"version":2}}))
            })
            .is_err()
    );
    assert!(
        behavior
            .observe(ProviderUpdate::ToolStatus {
                tool_id: "compact-1".into(),
                status: ToolCallStatus::Completed,
                metadata: None
            })
            .expect("no marker")
            .compaction
            .is_none()
    );
}

#[test]
fn malformed_annotation_containers_report_errors_without_hiding_valid_rows() {
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated);
    let valid = ProviderCommand {
        invocation_name: "review".into(),
        description: "Review".into(),
        input_hint: None,
        metadata: Some(json!({"codexRouter":{"kind":"skill"}})),
    };
    for malformed in [json!({"codexRouter":true}), json!(false)] {
        let invalid = ProviderCommand {
            invocation_name: "invalid".into(),
            description: "Invalid".into(),
            input_hint: None,
            metadata: Some(malformed.clone()),
        };
        let facts = behavior
            .observe(ProviderUpdate::Commands {
                commands: vec![valid.clone(), invalid],
                metadata: Some(json!({"codexRouter":{"skillStatus":"available"}})),
            })
            .expect("row errors are data");
        let Some(SkillView::Available { skills, errors }) = facts.skill_view else {
            panic!("valid source view");
        };
        assert_eq!(skills.len(), 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].reason,
            session_event_model::SkillRowErrorReason::MalformedMetadata
        );
        assert_eq!(
            behavior
                .observe(ProviderUpdate::Commands {
                    commands: vec![valid.clone()],
                    metadata: Some(malformed)
                })
                .expect("source failure data")
                .skill_view,
            Some(SkillView::Unavailable(SkillSourceError::MalformedMetadata))
        );
    }
}

/// Oracle: D1 R18 and V33, "Claude's settings carry the roots and the hook"
/// (wrong build: "the hook missing"): a session with work roots and a hook
/// command gets per-session settings naming every root as an additional
/// directory and the hook on every write and the shell; with no hook command
/// it gets none.
#[test]
fn work_roots_become_claude_session_settings_with_the_hook() {
    // Arrange
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Unavailable(
        SkillSourceError::UnestablishedClassification,
    ));
    let work_roots = acp_client_runtime::SessionWorkRoots {
        roots: vec![
            std::path::PathBuf::from("/scratch/s"),
            std::path::PathBuf::from("/tmp/agents"),
        ],
        ask_outside_command: Some("'/node' '/hook.ts' '[\"/scratch/s\"]'".to_owned()),
        router_auto: true,
    };
    let auto_without_hook = acp_client_runtime::SessionWorkRoots {
        router_auto: true,
        ..acp_client_runtime::SessionWorkRoots::default()
    };

    // Act
    let meta = behavior.session_new_meta(&work_roots);
    let none = behavior.session_new_meta(&acp_client_runtime::SessionWorkRoots::default());
    let marked = behavior.session_new_meta(&auto_without_hook);

    // Assert
    let settings = &meta.expect("Claude session settings")["codexRouter"]["workRootSettings"];
    assert_eq!(
        settings["permissions"]["additionalDirectories"],
        serde_json::json!(["/scratch/s", "/tmp/agents"])
    );
    let hook = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(hook["matcher"], "Write|Edit|MultiEdit|NotebookEdit|Bash");
    assert_eq!(hook["hooks"][0]["type"], "command");
    assert_eq!(
        hook["hooks"][0]["command"],
        "'/node' '/hook.ts' '[\"/scratch/s\"]'"
    );
    assert!(none.is_none(), "a named-mode start carries nothing");
    assert_eq!(
        marked.map(serde_json::Value::Object),
        Some(json!({"codexRouter": {"startMode": "auto"}})),
        "an auto start without its hook still says auto, so the entry refuses it"
    );
}
