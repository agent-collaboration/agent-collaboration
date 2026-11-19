use super::CursorAcpBehavior;
use acp_client_runtime::{ProviderBehavior, ProviderCommand, ProviderError, ProviderUpdate};
use session_event_model::{
    PromptContent, ProviderSkillScope, ProviderSubmission, SelectedSkill, SkillInvocationName,
    SkillRowErrorReason, SkillView,
};

#[test]
fn initialization_authentication_requires_cursors_documented_agent_method() {
    assert_eq!(
        CursorAcpBehavior.initialization_authentication(),
        acp_client_runtime::ProviderInitializationAuthentication::RequiredAgentMethod {
            method_id: "cursor_login"
        }
    );
}

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

/// Oracle: L6 and Cursor 2026.10.22's `formatOperation` — the title is read
/// only in the exact shapes Cursor writes; anything ambiguous reads nothing,
/// so the request asks.
#[test]
fn cursor_titles_are_read_only_in_their_exact_shapes() {
    use acp_client_runtime::{PermissionRequestAction, StatedPermissionTarget};
    let behavior = super::CursorAcpBehavior;
    let file = PermissionRequestAction::FileAccess;
    let shell = PermissionRequestAction::ShellCommand;
    let path = |text: &str| Some(StatedPermissionTarget::File(text.into()));
    let cases = [
        (file, "Delete `/s/a.txt`", path("/s/a.txt")),
        (file, "Read `/s/a b.txt`", path("/s/a b.txt")),
        (file, "Edit `/s/a.txt`", path("/s/a.txt")),
        (file, "Write /s/a.txt", None),
        (file, "Delete `/s/a` `/t/b`", None),
        (file, "Delete ``", None),
        (file, "Delete /s/a.txt", None),
        (file, "`rm /s/a.txt`", None),
        (
            shell,
            "`rm /s/a.txt`",
            Some(StatedPermissionTarget::Command("rm /s/a.txt".into())),
        ),
        (shell, "`echo \\`id\\``", None),
        (shell, "rm /s/a.txt", None),
        (shell, "Delete `/s/a.txt`", None),
    ];
    for (action, title, expected) in cases {
        assert_eq!(
            acp_client_runtime::ProviderBehavior::stated_permission_target(
                &behavior, action, title
            ),
            expected,
            "{title}"
        );
    }
}
