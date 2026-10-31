//! Account facts have one renderer without depending on either surface controller.
use super::quota_fact_presentation::{
    reset_pace_role, reset_pace_summary, sample_metadata_summary,
};
use agent_collaboration_tui::{
    UiText, UiTextRole,
    controls::{DialogSection, DialogSections, FormField, FormFields},
};
use codex_router_quota_service::quota_display_model::QuotaSelectedAccountViewModel;
use iocraft::prelude::*;

fn field(label: &str, value: String, role: UiTextRole) -> Element<'static, FormField> {
    element!(FormField(label:label.to_owned()) {UiText(content:value,role,wrap:TextWrap::Wrap)})
}

pub(crate) fn render_account_details(
    details: &QuotaSelectedAccountViewModel,
    width: usize,
    login_target: Option<&super::AccountActionTarget>,
) -> AnyElement<'static> {
    let label_width = FormFields::label_width_for([
        "Status", "Reason", "weekly", "5h", "sample", "rate", "conn", "clients", "guards", "reset",
        "note",
    ]);
    element!(View(width:u32::try_from(width).unwrap_or(u32::MAX),max_width:100pct) {DialogSections {
        DialogSection(heading:String::new()) {FormFields(stacked:width<32,minimum_label_width:Some(label_width)) {
            #(vec![field("Status",details.status.clone(),UiTextRole::Body),field("Reason",details.reason.clone(),UiTextRole::Body)])
            #(login_target.map(|target|field("Login",login_command_preview(target),UiTextRole::Secondary)))
        }}
        DialogSection(heading:"Quota windows".to_owned()) {FormFields(stacked:width<32,minimum_label_width:Some(label_width)) {
            #(vec![field("weekly",details.weekly_window.clone(),UiTextRole::Body),field("5h",details.short_window.clone(),UiTextRole::Body)])
        }}
        DialogSection(heading:"Reset pace".to_owned()) {FormFields(stacked:width<32,minimum_label_width:Some(label_width)) {
            #(vec![field("weekly",reset_pace_summary(&details.reset_pace),reset_pace_role(details.reset_pace.state)),field("sample",sample_metadata_summary(&details.sample_metadata),UiTextRole::Body),field("rate",details.total_rate.clone(),UiTextRole::Body),field("conn",details.connection_rate.clone(),UiTextRole::Body),field("5h",reset_pace_summary(&details.short_reset_pace),reset_pace_role(details.short_reset_pace.state))])
        }}
        DialogSection(heading:"Activity".to_owned()) {FormFields(stacked:width<32,minimum_label_width:Some(label_width)) {
            #(vec![field("clients",details.active_clients.clone(),UiTextRole::Body),field("guards",details.guards.clone(),UiTextRole::Body),field("reset",details.reset.clone(),UiTextRole::Body),field("note",details.note.clone(),UiTextRole::Secondary)])
        }}
    }}).into_any()
}

fn login_command_preview(target: &super::AccountActionTarget) -> String {
    let label = target.label.as_str();
    let quoted = if !label.is_empty()
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./".contains(character))
    {
        label.to_owned()
    } else {
        format!("'{}'", label.replace('\'', "'\\''"))
    };
    format!(
        "codex-router account login --provider {} --label {quoted}",
        target.provider
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn login_preview_quotes_the_captured_label_and_uses_its_provider() {
        let mut row = crate::quota_browse_presentation_test::quota_two_account_view_model()
            .rows
            .remove(0);
        row.account = "O'Neil Research $literal".to_owned();
        row.provider = codex_router_core::provider::Provider::Claude;
        let target = crate::AccountActionTarget::from_row(&row, 0).unwrap();
        assert_eq!(
            super::login_command_preview(&target),
            "codex-router account login --provider claude --label 'O'\\''Neil Research $literal'"
        );
    }
}
