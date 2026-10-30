use acp_client_runtime::{ProviderBehavior, ProviderCommand, ProviderError, ProviderUpdate};
use acp_provider_adapters::CursorAcpBehavior;
use session_event_model::{
    PromptContent, ProviderSkillScope, ProviderSubmission, SelectedSkill, SkillInvocationName,
    SkillRowErrorReason, SkillView,
};

fn command(name: &str, description: &str) -> ProviderCommand {
    ProviderCommand {
        invocation_name: name.into(),
        description: description.into(),
        input_hint: None,
        metadata: None,
    }
}

#[test]
fn actual_cursor_suffixes_keep_valid_rows_beside_closed_errors() {
    let facts = CursorAcpBehavior
        .observe(ProviderUpdate::Commands {
            commands: vec![
                command("project", "Project work (project skill)"),
                command("user", "User work (user skill)"),
                command("builtin", "Builtin work (builtin skill)"),
                command("generic", "Generic work (skill)"),
                command("spoof", "Pretend (project skill) (project)"),
                command("copy-request-id", "Copy request ID"),
                command("nested/id", "Uninvocable (project skill)"),
                command("duplicate", "Skill (skill)"),
                command("duplicate", "Command (user)"),
                command("unknown", "Unknown (future skill)"),
            ],
            metadata: None,
        })
        .expect("derived facts");
    let Some(SkillView::Available { skills, errors }) = facts.skill_view else {
        panic!("available current inventory");
    };
    assert_eq!(
        skills
            .iter()
            .map(|row| (
                row.invocation_name.as_str(),
                row.description.as_str(),
                row.scope
            ))
            .collect::<Vec<_>>(),
        vec![
            ("project", "Project work", ProviderSkillScope::Repo),
            ("user", "User work", ProviderSkillScope::User),
            ("builtin", "Builtin work", ProviderSkillScope::System),
            ("generic", "Generic work", ProviderSkillScope::System),
        ]
    );
    assert_eq!(
        errors
            .iter()
            .map(|row| (row.advertised_name.as_str(), row.reason))
            .collect::<Vec<_>>(),
        vec![
            ("nested/id", SkillRowErrorReason::UninvocableName),
            ("duplicate", SkillRowErrorReason::AmbiguousName),
            ("unknown", SkillRowErrorReason::UnrecognizedMetadata),
        ]
    );
}

#[test]
fn cursor_rewrites_all_selected_full_tokens_with_exact_text_and_images() {
    let image =
        PromptContent::image("image/png".into(), "aGVsbG8=".into(), None).expect("fixture image");
    let submission = ProviderSubmission {
        content: vec![
            PromptContent::text(
                "前置\t$review  α\n$other\t$review $review_suffix $unselected".into(),
            )
            .expect("text"),
            image.clone(),
        ],
        selected: vec![
            SelectedSkill {
                invocation_name: SkillInvocationName::new("review").expect("name"),
            },
            SelectedSkill {
                invocation_name: SkillInvocationName::new("other").expect("name"),
            },
            SelectedSkill {
                invocation_name: SkillInvocationName::new("review").expect("duplicate name"),
            },
        ],
    };
    assert_eq!(
        CursorAcpBehavior.prepare(submission).expect("prepared"),
        vec![
            PromptContent::text(
                "前置\t/review  α\n/other\t/review $review_suffix $unselected".into()
            )
            .expect("literal expected"),
            image
        ]
    );
}

#[test]
fn cursor_refuses_unaddressable_and_multi_text_selections() {
    for contents in [
        vec!["prefix$review"],
        vec!["$review_suffix"],
        vec!["$review", "second text"],
    ] {
        let submission = ProviderSubmission {
            content: contents
                .iter()
                .map(|text| PromptContent::text((*text).into()).expect("fixture text"))
                .collect(),
            selected: vec![SelectedSkill {
                invocation_name: SkillInvocationName::new("review").expect("name"),
            }],
        };
        assert!(matches!(
            CursorAcpBehavior.prepare(submission),
            Err(ProviderError::InvalidSelection | ProviderError::Unsupported)
        ));
    }
    assert_eq!(CursorAcpBehavior.compact(), Err(ProviderError::Unsupported));
}
