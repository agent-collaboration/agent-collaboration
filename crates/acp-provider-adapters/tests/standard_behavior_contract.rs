use acp_client_runtime::{ProviderBehavior, ProviderError, ProviderUpdate};
use acp_provider_adapters::StandardAcpBehavior;
use session_event_model::{
    PromptContent, ProviderSubmission, SelectedSkill, SkillInvocationName, SkillSourceError,
    SkillView,
};

#[test]
fn standard_preserves_ordinary_content_and_refuses_unestablished_effects() {
    let behavior: &dyn ProviderBehavior = &StandardAcpBehavior;
    let content = vec![
        PromptContent::text(" untouched $review \n".into()).expect("fixture text"),
        PromptContent::image("image/png".into(), "aGVsbG8=".into(), None).expect("fixture image"),
    ];
    assert_eq!(
        behavior.prepare(ProviderSubmission::plain(content.clone())),
        Ok(content.clone())
    );
    let selected = SelectedSkill {
        invocation_name: SkillInvocationName::new("review").expect("fixture name"),
    };
    assert_eq!(
        behavior.prepare(ProviderSubmission {
            content,
            selected: vec![selected]
        }),
        Err(ProviderError::SourceUnavailable)
    );
    assert_eq!(behavior.compact(), Err(ProviderError::Unsupported));
    assert_eq!(
        behavior
            .observe(ProviderUpdate::SessionAdmitted)
            .expect("admission facts")
            .skill_view,
        Some(SkillView::Unavailable(
            SkillSourceError::UnestablishedClassification
        ))
    );
}
