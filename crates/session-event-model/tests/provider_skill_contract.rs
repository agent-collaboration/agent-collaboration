use session_event_model::{
    InvalidProviderSkill, ProviderSkill, ProviderSkillScope, SelectedSkill, SkillInvocationName,
    SkillSourceError, SkillView,
};

fn selection(name: &str) -> Result<SelectedSkill, InvalidProviderSkill> {
    Ok(SelectedSkill {
        invocation_name: SkillInvocationName::new(name)?,
    })
}

#[test]
fn invocation_identity_rejects_empty_or_nul_and_preserves_unicode() {
    assert!(SkillInvocationName::new("").is_err());
    assert!(SkillInvocationName::new("bad\0name").is_err());
    assert_eq!(
        SkillInvocationName::new("组织:skill")
            .expect("Unicode name")
            .as_str(),
        "组织:skill"
    );
}

#[test]
fn dispatch_requires_one_exact_current_invocation_in_owning_session() {
    let selected = selection("review").expect("fixture selection");
    let skill = ProviderSkill {
        invocation_name: selected.invocation_name.clone(),
        description: "Review changes".into(),
        scope: ProviderSkillScope::System,
    };
    let view = SkillView::Available {
        skills: vec![skill.clone()],
        errors: vec![],
    };
    assert!(view.contains_selection(&selected));
    assert!(!view.contains_selection(&selection("other").expect("other selection")));
    assert!(!SkillView::Unknown.contains_selection(&selected));
    assert!(
        !SkillView::Unavailable(SkillSourceError::ProviderUnavailable)
            .contains_selection(&selected)
    );
    assert!(
        !SkillView::Available {
            skills: vec![],
            errors: vec![]
        }
        .contains_selection(&selected)
    );
    assert!(
        !SkillView::Available {
            skills: vec![skill.clone(), skill],
            errors: vec![]
        }
        .contains_selection(&selected)
    );
}
