//! Pure lowering tests; real binding/native execution are separate proof layers.
use super::*;
use collaboration_service::CommandContent;
use session_event_model::{SessionItem, SessionItemKind};

fn lower_selection(
    content: &[CommandContent],
    selections: &[SelectedItem],
    skills: &[NativeSkill],
    commands: &[String],
) -> Result<Vec<CommandContent>, SelectionFailure> {
    super::lower_selection(
        content,
        selections,
        skills,
        commands,
        ProviderKind::ClaudeCode,
    )
}

#[test]
fn native_capabilities_cursor_selected_arguments_cannot_activate_another_native_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let skills = vec![
        skill("chosen", "/native/chosen/SKILL.md")?,
        skill("other", "/native/other/SKILL.md")?,
    ];
    let selected = vec![selection("chosen", "/native/chosen/SKILL.md")?];
    let commands = vec!["chosen".into(), "other".into(), "builtin".into()];
    for text in [
        "$chosen /other",
        "$chosen\n/other:suffix",
        "$chosen /chosen.extra",
        "$chosen /builtin",
    ] {
        assert_eq!(
            super::lower_selection(
                &[CommandContent::text(text.into())?],
                &selected,
                &skills,
                &commands,
                ProviderKind::Cursor
            ),
            Err(SelectionFailure::UnsupportedForm)
        );
    }
    assert_eq!(
        super::lower_selection(
            &[CommandContent::text("$chosen  λ\n$HOME /unknown".into())?],
            &selected,
            &skills,
            &commands,
            ProviderKind::Cursor
        )?,
        vec![CommandContent::text("/chosen  λ\n$HOME /unknown".into())?]
    );
    Ok(())
}

fn skill(name: &str, path: &str) -> Result<NativeSkill, CatalogueReadFailure> {
    Ok(NativeSkill {
        id: NativeSkillId::new(path.into())?,
        name: CapabilityName::new(name.into())?,
        description: "fixture".into(),
        origin: NativeOrigin::Filesystem {
            actual_path: CapabilityPath::new(path.into())?,
        },
        native_scope: NativeScope::Workspace,
        availability: NativeAvailability::Available,
        display_version: None,
        plugin_id: None,
        lowering: NativeSkillLowering::Qualified {
            canonical_command: NativeCommandName::new(name.into())?,
        },
    })
}

fn selection(name: &str, path: &str) -> Result<SelectedItem, CatalogueReadFailure> {
    Ok(SelectedItem::Skill {
        submitted_name: CapabilityName::new(name.into())?,
        submitted_path: CapabilityPath::new(path.into())?,
    })
}

#[test]
fn native_capabilities_selected_lowering_preserves_arguments_and_dollar_literals()
-> Result<(), Box<dyn std::error::Error>> {
    let skills = vec![
        skill("chosen", "/native/chosen/SKILL.md")?,
        skill("other", "/native/other/SKILL.md")?,
    ];
    let selected = vec![selection("chosen", "/native/chosen/SKILL.md")?];
    for (submitted, expected) in [
        ("$chosen", "/chosen"),
        ("$chosen  λ\n$HOME $$TAIL\n", "/chosen  λ\n$HOME $$TAIL\n"),
        (
            "$chosen\targument $other-extra",
            "/chosen\targument $other-extra",
        ),
    ] {
        assert_eq!(
            lower_selection(
                &[CommandContent::text(submitted.into())?],
                &selected,
                &skills,
                &["chosen".into()]
            )?,
            vec![CommandContent::text(expected.into())?]
        );
    }
    Ok(())
}

#[test]
fn native_capabilities_selected_lowering_rejects_ambiguous_or_changed_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let original = skill("chosen", "/native/chosen/SKILL.md")?;
    let selected = vec![selection("chosen", "/native/chosen/SKILL.md")?];
    let content = vec![CommandContent::text("$chosen args".into())?];
    let advertised = vec!["chosen".into()];
    let mut disabled = original.clone();
    disabled.availability = NativeAvailability::Disabled;
    let mut unknown = original.clone();
    unknown.availability = NativeAvailability::Unknown;
    let mut unqualified = original.clone();
    unqualified.lowering = NativeSkillLowering::Unqualified {
        reason: LoweringUnsupported::ScopeUnqualified,
    };
    let cases = [
        (vec![], advertised.clone(), SelectionFailure::Unresolved),
        (
            vec![disabled],
            advertised.clone(),
            SelectionFailure::Disabled,
        ),
        (
            vec![unknown],
            advertised.clone(),
            SelectionFailure::Unresolved,
        ),
        (
            vec![unqualified],
            advertised.clone(),
            SelectionFailure::UnsupportedForm,
        ),
        (
            vec![skill("chosen", "/foreign/SKILL.md")?],
            advertised.clone(),
            SelectionFailure::WrongContext,
        ),
        (
            vec![original.clone(), skill("chosen", "/shadowed/SKILL.md")?],
            advertised,
            SelectionFailure::Ambiguous,
        ),
        (vec![original.clone()], vec![], SelectionFailure::Unresolved),
        (
            vec![original],
            vec!["chosen".into(), "chosen".into()],
            SelectionFailure::Ambiguous,
        ),
    ];
    for (skills, commands, expected) in cases {
        assert_eq!(
            lower_selection(&content, &selected, &skills, &commands),
            Err(expected)
        );
    }
    Ok(())
}

#[test]
fn native_capabilities_selected_lowering_rejects_nonleading_duplicate_and_multiple_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let skills = vec![
        skill("chosen", "/native/chosen/SKILL.md")?,
        skill("other", "/native/other/SKILL.md")?,
    ];
    let selected = vec![selection("chosen", "/native/chosen/SKILL.md")?];
    for text in [
        "ordinary",
        "before $chosen",
        "$chosen-extra",
        "$chosen\n$chosen args",
        "$chosen\t$other",
    ] {
        assert_eq!(
            lower_selection(
                &[CommandContent::text(text.into())?],
                &selected,
                &skills,
                &["chosen".into()]
            ),
            Err(SelectionFailure::UnsupportedForm),
            "{text}"
        );
    }
    let content = vec![CommandContent::text("$chosen args".into())?];
    assert_eq!(
        lower_selection(&content, &[], &skills, &["chosen".into()]),
        Err(SelectionFailure::UnsupportedForm)
    );
    assert_eq!(
        lower_selection(
            &content,
            &[
                selected.first().ok_or("selection")?.clone(),
                selection("other", "/native/other/SKILL.md")?
            ],
            &skills,
            &["chosen".into()]
        ),
        Err(SelectionFailure::UnsupportedForm)
    );
    Ok(())
}

#[test]
fn native_capabilities_retained_command_lists_replace_and_preserve_prose() {
    let notice = |sequence, text: &str, kind| collaboration_service::HubEvent {
        sequence,
        event: SessionEvent::ItemStarted {
            item: SessionItem {
                item_id: format!("item-{sequence}"),
                kind,
                text: Some(text.into()),
            },
        },
    };
    let old = notice(
        1,
        r#"{"availableCommands":[{"name":"old","description":"Old"}]}"#,
        SessionItemKind::Notice,
    );
    let new = notice(
        2,
        r#"{"availableCommands":[{"name":"new","description":"New"}]}"#,
        SessionItemKind::Notice,
    );
    let prose = notice(
        3,
        r#"{"availableCommands":[],"message":"Keep this diagnostic"}"#,
        SessionItemKind::Notice,
    );
    let assistant = notice(
        4,
        r#"{"availableCommands":[]}"#,
        SessionItemKind::AgentMessage,
    );
    assert_eq!(
        latest_commands(&[old, new, prose, assistant]),
        Some(vec!["new".into()])
    );
    assert_eq!(
        latest_commands(&[notice(
            5,
            r#"{"availableCommands":[]}"#,
            SessionItemKind::Notice
        )]),
        Some(vec![])
    );
    assert_eq!(latest_commands(&[]), None);
}
