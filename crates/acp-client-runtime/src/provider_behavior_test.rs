use super::{PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate};
use session_event_model::{PromptContent, ProviderSubmission, SkillView};
use std::sync::Arc;

struct LiteralBehavior {
    output: &'static str,
    view: SkillView,
}

impl ProviderBehavior for LiteralBehavior {
    fn can_reuse_setting_confirmation(&self, _kind: crate::ProviderSettingKind) -> bool {
        false
    }
    fn model_metadata_source(&self) -> crate::ModelMetadataSource {
        crate::ModelMetadataSource::Unavailable
    }
    fn decode_model_metadata(&self, _value: &serde_json::Value) -> crate::ModelReasoningMetadata {
        crate::ModelReasoningMetadata::Unknown(crate::ModelReasoningUnknown::UnsupportedSource)
    }
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
