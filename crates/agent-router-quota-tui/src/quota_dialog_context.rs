//! One presentation projection of the caller-owned captured dialog target.
use super::{
    quota_account_options::AccountOptionsState, quota_floor_editor::WeeklyFloorEditorState,
    quota_reset_presentation_model::ResetPaneTarget,
};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusAccountViewModel;
use iocraft::prelude::*;

pub(crate) struct QuotaDialogIdentity {
    label: String,
    tag: Option<String>,
}

impl QuotaDialogIdentity {
    pub(crate) fn full_label(&self) -> String {
        self.tag.as_ref().map_or_else(
            || self.label.clone(),
            |tag| format!("{} [{tag}]", self.label),
        )
    }

    pub(crate) fn render(&self, width: usize) -> AnyElement<'static> {
        let tag = self.tag.as_ref().map(|tag| format!("[{tag}]"));
        use unicode_width::UnicodeWidthStr;
        let label_width =
            width.saturating_sub(tag.as_ref().map_or(0, |tag| tag.as_str().width() + 1));
        let label = if self.label.as_str().width() <= label_width {
            self.label.clone()
        } else {
            let mut prefix = String::new();
            for character in self.label.chars() {
                let previous_length = prefix.len();
                prefix.push(character);
                if prefix.as_str().width() > label_width.saturating_sub(1) {
                    prefix.truncate(previous_length);
                    break;
                }
            }
            if label_width > 0 {
                prefix.push('…');
            }
            prefix
        };
        element! {View(width:u32::try_from(width).unwrap_or(u32::MAX),max_width:100pct,column_gap:1,flex_shrink:0.0_f32) {
            View(min_width:0,flex_grow:1.0_f32,flex_shrink:1.0_f32,overflow:Overflow::Hidden) {Text(content:label,color:Color::White,wrap:TextWrap::NoWrap)}
            #(tag.map(|content|element!(View(flex_shrink:0.0_f32) {Text(content,color:Color::Grey,wrap:TextWrap::NoWrap)})))
        }}.into_any()
    }
}

pub(crate) fn current_dialog_identity(
    options: Option<&AccountOptionsState>,
    reset: Option<&ResetPaneTarget>,
    floor: Option<&WeeklyFloorEditorState>,
    details: Option<&AccountId>,
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
    details
        .and_then(|identity| rows.iter().find(|row| &row.account_id == identity))
        .map(|row| QuotaDialogIdentity {
            label: row.account.clone(),
            tag: Some(row.account_tag.clone()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_identity_marks_only_truncated_names_and_keeps_full_tag_with_cjk_budget() {
        for label in ["ssdev", "東京", "東京 Aurora Research Account Alpha"] {
            let identity = QuotaDialogIdentity {
                label: label.to_owned(),
                tag: Some("full-tag".to_owned()),
            };
            let canvas = identity.render(22).render(None);
            let frame = canvas.to_string();
            assert!(frame.contains("[full-tag]"), "full tag reserved: {frame}");
            assert_eq!(canvas.width(), 22, "{frame}");
            assert_eq!(identity.full_label(), format!("{label} [full-tag]"));
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
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 22)
            );
        }
    }
}
