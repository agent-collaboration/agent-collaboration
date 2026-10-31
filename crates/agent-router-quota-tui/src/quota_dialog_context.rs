//! One presentation projection of the caller-owned captured dialog target.
use super::{
    quota_account_options::AccountOptionsState, quota_floor_editor::WeeklyFloorEditorState,
    quota_reset_presentation_model::ResetPaneTarget,
};
use agent_collaboration_tui::controls::PinnedContext;
use codex_router_quota_service::quota_display_model::QuotaStatusAccountViewModel;

pub(crate) struct QuotaDialogIdentity {
    label: String,
    tag: Option<String>,
}

impl QuotaDialogIdentity {
    #[cfg(test)]
    pub(crate) fn full_label(&self) -> String {
        self.tag.as_ref().map_or_else(
            || self.label.clone(),
            |tag| format!("{} [{tag}]", self.label),
        )
    }

    pub(crate) fn pinned_context(self) -> PinnedContext {
        let context = PinnedContext::new(self.label);
        match self.tag {
            Some(tag) => context.with_tag(tag),
            None => context,
        }
    }
}

pub(crate) fn current_dialog_identity(
    options: Option<&AccountOptionsState>,
    reset: Option<&ResetPaneTarget>,
    floor: Option<&WeeklyFloorEditorState>,
    details: Option<&super::AccountActionTarget>,
    rows: &[QuotaStatusAccountViewModel],
) -> Option<QuotaDialogIdentity> {
    if let Some(target) = reset {
        return Some(QuotaDialogIdentity {
            label: target.account_label.clone(),
            tag: Some(target.account_tag.clone()),
        });
    }
    if let Some(options) = options {
        return Some(QuotaDialogIdentity {
            label: options.target.account_label.clone(),
            tag: Some(options.target.account_tag.clone()),
        });
    }
    if let Some(editor) = floor {
        return Some(QuotaDialogIdentity {
            label: editor.account_label.clone(),
            tag: rows
                .iter()
                .find(|row| row.account_id == editor.account_id)
                .map(|row| row.account_tag.clone()),
        });
    }
    details.map(|target| QuotaDialogIdentity {
        label: target.label.as_str().to_owned(),
        tag: rows
            .iter()
            .find(|row| row.account_id == target.account_id)
            .map(|row| row.account_tag.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        quota_browse_presentation_test::quota_two_account_view_model,
        test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
    };
    use iocraft::prelude::*;

    // Lead 01a18f3f-dbee: fitting moved to the shared frame; quota passes raw identity.
    #[tokio::test]
    async fn captured_identity_marks_only_truncated_names_and_keeps_full_tag_with_cjk_budget() {
        for label in ["ssdev", "東京", "東京 Aurora Research Account Alpha"] {
            let identity = QuotaDialogIdentity {
                label: label.to_owned(),
                tag: Some("full-tag".to_owned()),
            };
            assert_eq!(identity.full_label(), format!("{label} [full-tag]"));
            let pinned = identity.pinned_context();
            assert_eq!(pinned.text, label, "consumer never pre-truncates identity");
            assert_eq!(pinned.tag.as_deref(), Some("full-tag"));
            let mut model = quota_two_account_view_model();
            model.rows[0].account = label.to_owned();
            model.rows[0].details.account = label.to_owned();
            model.rows[0].account_tag = "full-tag".to_owned();
            let mut fixture = quota_control_fixture(QuotaFixtureOptions {
                view_model: model,
                ports: Default::default(),
                width: 32,
                height: 24,
                control: QuotaFixtureControl::Details,
            });
            let events = futures_util::stream::pending::<TerminalEvent>();
            let frames = fixture
                .element
                .mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(frames);
            let canvas = crate::quota_frame_observation::next_matching_frame(
                frames.as_mut(),
                std::time::Duration::from_secs(2),
                |canvas| canvas.to_string().contains("[full-tag]"),
            )
            .await
            .unwrap();
            let frame = canvas.to_string();
            assert!(frame.contains("[full-tag]"), "full tag reserved: {frame}");
            assert_eq!(canvas.width(), 32, "{frame}");
            if label.contains("Research") {
                assert!(
                    frame.contains('…'),
                    "actual name truncation must be explicit: {frame}"
                );
                assert!(frame.contains("東京"), "CJK prefix stays whole: {frame}");
            } else {
                assert!(
                    frame.contains(label) && !frame.contains('…'),
                    "fitting names stay unchanged: {frame}"
                );
            }
            assert!(
                frame
                    .lines()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 32)
            );
        }
    }
}
