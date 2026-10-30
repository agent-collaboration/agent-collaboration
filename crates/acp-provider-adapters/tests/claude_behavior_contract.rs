use acp_client_runtime::{
    CompactionTrigger, PreparedCompact, ProviderBehavior, ProviderCommand, ProviderError,
    ProviderUpdate,
};
use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource};
use serde_json::json;
use session_event_model::{
    PromptContent, ProviderSubmission, SelectedSkill, SkillInvocationName, SkillSourceError,
    SkillView, ToolCallStatus,
};

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
