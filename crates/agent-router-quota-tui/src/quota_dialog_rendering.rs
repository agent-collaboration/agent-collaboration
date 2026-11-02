//! The reset owner and the dialog use the same bounded body height.
use agent_collaboration_tui::DialogPresentation;

pub(crate) fn quota_dialog_panel_height(viewport_height: usize, content_height: usize) -> usize {
    let height = u16::try_from(viewport_height).unwrap_or(u16::MAX);
    let (_, (_, frame_height)) = DialogPresentation::default().resolve(0, height.saturating_sub(2));
    let body_height = usize::from(frame_height.saturating_sub(6));
    content_height.min(body_height)
}
