use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission, SkillView};
use std::sync::Arc;

struct LiteralBehavior {
    output: &'static str,
    view: SkillView,
}

impl ProviderBehavior for LiteralBehavior {
    fn observe(&self, _: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        Ok(ProviderFacts {
            skill_view: Some(self.view.clone()),
            compaction: None,
        })
    }

    fn prepare(&self, _: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        Ok(vec![
            PromptContent::text(self.output.into())
                .map_err(|_| ProviderError::MalformedMetadata)?,
        ])
    }

    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}

#[test]
fn immutable_object_safe_boundary_preserves_different_implementations() {
    let behaviors: [Arc<dyn ProviderBehavior>; 2] = [
        Arc::new(LiteralBehavior {
            output: "first provider",
            view: SkillView::Unknown,
        }),
        Arc::new(LiteralBehavior {
            output: "second provider",
            view: SkillView::Available {
                skills: vec![],
                errors: vec![],
            },
        }),
    ];
    for (behavior, expected, view) in [
        (&behaviors[0], "first provider", SkillView::Unknown),
        (
            &behaviors[1],
            "second provider",
            SkillView::Available {
                skills: vec![],
                errors: vec![],
            },
        ),
    ] {
        assert_eq!(
            behavior
                .prepare(ProviderSubmission::plain(vec![]))
                .expect("prepared literal"),
            vec![PromptContent::text(expected.into()).expect("expected literal")]
        );
        assert_eq!(
            behavior
                .observe(ProviderUpdate::SessionClosed)
                .expect("facts")
                .skill_view,
            Some(view)
        );
        assert_eq!(behavior.compact(), Err(ProviderError::Unsupported));
    }
}
