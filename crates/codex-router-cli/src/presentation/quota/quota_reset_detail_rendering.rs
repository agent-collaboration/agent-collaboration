use codex_router_tui::{UiHeading, UiHeadingLevel};
use iocraft::prelude::*;

use crate::quota_reset::reset_session_supervisor::ConfirmationSelection;
use crate::quota_reset::reset_session_supervisor::KnownConsumeOutcome;
use crate::quota_reset::reset_session_supervisor::ResetCreditDisplayStatusDto;
use crate::quota_reset::reset_session_supervisor::ResetValueProvenance;
use crate::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot;
use crate::quota_reset::reset_session_supervisor::WorkflowPhase;
use crate::quota_reset::reset_session_supervisor::WorkflowResult;

use super::quota_browse_rendering::fit_line;
use super::quota_reset_detail_content::{
    activity_field, credit_status_label, disabled_reason, format_credit_expiry,
    format_credit_expiry_value, heading, muted_text, normal, saved_credit_count, warning_text,
};
use super::quota_reset_presentation_model::ResetPaneTarget;
use super::responsive_quota_layout::quota_spinner_tick;

const DETAIL_LABEL_WIDTH: usize = 20;
const CREDIT_MARKER_WIDTH: usize = 10;
const CREDIT_ID_WIDTH: usize = 12;

pub(super) struct ResetDetailDocument {
    pub(super) title: StyledText,
    pub(super) account: Option<StyledText>,
    pub(super) sections: Vec<ResetDetailSection>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ResetSectionRole {
    Content,
    Inventory,
    Decision,
    Result,
}

pub(super) struct ResetDetailSection {
    role: ResetSectionRole,
    pub(super) title: StyledText,
    pub(super) rows: Vec<ResetDetailRow>,
}

pub(super) enum ResetDetailRow {
    Text(StyledText),
    Field {
        label: String,
        value: String,
        value_color: Color,
    },
    Credit {
        marker: String,
        id_hint: String,
        description: String,
        marker_color: Color,
    },
    Choices {
        no_label: StyledText,
        yes_label: StyledText,
    },
    Spacer,
}

pub(super) struct StyledText {
    pub(super) content: String,
    pub(super) color: Color,
    pub(super) weight: Weight,
}

impl ResetDetailDocument {
    fn content_height(&self) -> usize {
        let title_rows = 1 + usize::from(self.account.is_some());
        let section_rows = self
            .sections
            .iter()
            .map(|section| 2 + section.rows.len())
            .sum::<usize>();
        title_rows + section_rows + 2
    }
}

pub(super) fn reset_panel_content_height(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    inventory_page_start: usize,
) -> usize {
    reset_detail_document(snapshot, target, inventory_page_start, 4, "⠋").content_height()
}

pub(super) fn reset_options_content_height(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    inventory_page_start: usize,
) -> usize {
    let document = reset_detail_document(snapshot, target, inventory_page_start, 4, "⠋");
    let result_title_rows = usize::from(snapshot.phase() == WorkflowPhase::Result);
    let section_rows = document
        .sections
        .iter()
        .map(|section| 2 + section.rows.len())
        .sum::<usize>();
    result_title_rows + section_rows
}

pub(super) fn render_reset_panel(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    width: usize,
    height: usize,
    inventory_page_start: usize,
    inventory_page_size: usize,
    spinner_tick: usize,
) -> AnyElement<'static> {
    let document = reset_detail_document(
        snapshot,
        target,
        inventory_page_start,
        inventory_page_size,
        quota_spinner_tick(spinner_tick),
    );
    let inner_width = width.saturating_sub(4).max(12);
    let title = render_styled_text(document.title, inner_width);
    let account = document
        .account
        .map(|text| render_styled_text(text, inner_width));
    let mut choices = None;
    let sections = document
        .sections
        .into_iter()
        .map(|section| render_section(section, inner_width, false, &mut choices))
        .collect::<Vec<_>>();

    element! {
        View(
            width: u32::try_from(width).unwrap_or(u32::MAX),
            height: u32::try_from(height).unwrap_or(u32::MAX),
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: Color::DarkGrey,
            overflow: Overflow::Hidden,
            padding_left: 1,
            padding_right: 1,
        ) {
            #(title)
            #(account)
            #(sections)
        }
    }
    .into_any()
}

pub(super) fn render_reset_panel_content(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    width: usize,
    inventory_page_start: usize,
    inventory_page_size: usize,
    spinner_tick: usize,
    mut confirmation_choices: Option<AnyElement<'static>>,
) -> AnyElement<'static> {
    let document = reset_detail_document(
        snapshot,
        target,
        inventory_page_start,
        inventory_page_size,
        quota_spinner_tick(spinner_tick),
    );
    let title = (snapshot.phase() == WorkflowPhase::Result).then(|| {
        element!(Text(
            content: document.title.content,
            color: document.title.color,
            weight: document.title.weight,
            wrap: TextWrap::Wrap,
        ))
        .into_any()
    });
    let sections = document
        .sections
        .into_iter()
        .map(|section| {
            render_section(
                section,
                width.saturating_sub(2),
                true,
                &mut confirmation_choices,
            )
        })
        .collect::<Vec<_>>();
    element! {
        View(width: u32::try_from(width).unwrap_or(u32::MAX),max_width:100pct,flex_shrink:0.0_f32,flex_direction: FlexDirection::Column) {
            #(title)
            #(sections)
        }
    }
    .into_any()
}

fn render_section(
    section: ResetDetailSection,
    width: usize,
    natural: bool,
    choices: &mut Option<AnyElement<'static>>,
) -> AnyElement<'static> {
    let reveal = natural && section.role == ResetSectionRole::Inventory;
    let decision = natural && section.role == ResetSectionRole::Decision;
    let show_title = !(decision || natural && section.role == ResetSectionRole::Result);
    let title = show_title.then(|| {
        if natural && section.role == ResetSectionRole::Inventory {
            element!(Text(content: section.title.content, color: Color::White, weight: Weight::Bold, wrap: TextWrap::Wrap)).into_any()
        } else {
            element!(UiHeading(content: section.title.content, level: UiHeadingLevel::Section)).into_any()
        }
    });
    let rows = section
        .rows
        .into_iter()
        .map(|row| render_detail_row(row, width, natural, choices))
        .collect::<Vec<_>>();
    element! {
        View(
            width: 100pct,
            flex_direction: FlexDirection::Column,
            margin_top: if decision {0}else {1},
        ) {
            codex_router_tui::FocusReveal(has_focus:reveal) {#(title) #(rows)}
        }
    }
    .into_any()
}

fn render_detail_row(
    row: ResetDetailRow,
    width: usize,
    natural: bool,
    choices: &mut Option<AnyElement<'static>>,
) -> AnyElement<'static> {
    match row {
        ResetDetailRow::Text(text) if natural=>element!(Text(content:text.content,color:text.color,weight:text.weight,wrap:TextWrap::Wrap)).into_any(),
        ResetDetailRow::Text(text) => render_styled_text(text, width),
        ResetDetailRow::Field {
            label,
            value,
            value_color,
        } => {
            if natural {return element!(codex_router_tui::FormField(label,label_width:u16::try_from(DETAIL_LABEL_WIDTH).unwrap_or(u16::MAX),stacked:width<52) {Text(content:value,color:value_color,wrap:TextWrap::Wrap)}).into_any();}
            let value_width = width.saturating_sub(DETAIL_LABEL_WIDTH);
            element! {
                View(width: 100pct) {
                    View(width: u32::try_from(DETAIL_LABEL_WIDTH).unwrap_or(u32::MAX)) {
                        Text(content: fit_line(&label, DETAIL_LABEL_WIDTH), color: Color::Grey, wrap: TextWrap::NoWrap)
                    }
                    Text(content: fit_line(&value, value_width), color: value_color, wrap: TextWrap::NoWrap)
                }
            }
            .into_any()
        }
        ResetDetailRow::Credit {
            marker,
            id_hint,
            description,
            marker_color,
        } => {
            if natural && width<56 {
                return element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                    View(column_gap:1,flex_shrink:0.0_f32) {
                        Text(content:marker,color:marker_color,wrap:TextWrap::NoWrap)
                        Text(content:id_hint,color:Color::White,wrap:TextWrap::NoWrap)
                    }
                    Text(content:description,color:Color::White,wrap:TextWrap::Wrap)
                }}.into_any();
            }
            let description_width = width
                .saturating_sub(CREDIT_MARKER_WIDTH)
                .saturating_sub(CREDIT_ID_WIDTH);
            element! {
                View(width: 100pct,column_gap:if natural {2}else {0}) {
                    View(width: u32::try_from(CREDIT_MARKER_WIDTH).unwrap_or(u32::MAX)) {
                        Text(content: if natural {marker}else {fit_line(&marker, CREDIT_MARKER_WIDTH)}, color: marker_color, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                    }
                    View(width: u32::try_from(CREDIT_ID_WIDTH).unwrap_or(u32::MAX)) {
                        Text(content: if natural {id_hint}else {fit_line(&id_hint, CREDIT_ID_WIDTH)}, color: Color::White, wrap: TextWrap::NoWrap)
                    }
                    View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_width:0) {Text(content: if natural {description}else {fit_line(&description, description_width)}, color: Color::White, wrap: if natural {TextWrap::Wrap}else {TextWrap::NoWrap})}
                }
            }
            .into_any()
        }
        ResetDetailRow::Choices {
            no_label,
            yes_label,
        } => {
            if let Some(controls)=choices.take() {return controls;}
            element! {
            View(width:100pct,flex_direction:FlexDirection::Column) {
            codex_router_tui::UiText(content:"1 credit. Cannot undo.".to_owned(),role:codex_router_tui::UiTextRole::Warning,wrap:TextWrap::Wrap)
            View(width: 100pct, column_gap: 4) {
                Text(content: no_label.content, color: no_label.color, weight: no_label.weight, wrap: TextWrap::NoWrap)
                Text(content: yes_label.content, color: yes_label.color, weight: yes_label.weight, wrap: TextWrap::NoWrap)
            }
            }
        }
        .into_any()
        },
        ResetDetailRow::Spacer => element! { Text(content: "") }.into_any(),
    }
}

fn render_styled_text(text: StyledText, width: usize) -> AnyElement<'static> {
    element! {
        Text(
            content: fit_line(&text.content, width),
            color: text.color,
            weight: text.weight,
            wrap: TextWrap::NoWrap,
        )
    }
    .into_any()
}

fn reset_detail_document(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    inventory_page_start: usize,
    inventory_page_size: usize,
    spinner: &str,
) -> ResetDetailDocument {
    if snapshot.phase() == WorkflowPhase::Result {
        return result_document(snapshot.result());
    }
    let sections = match snapshot.phase() {
        WorkflowPhase::Browse | WorkflowPhase::Result => Vec::new(),
        WorkflowPhase::Inspecting => inspection_sections(snapshot, spinner),
        WorkflowPhase::Inspected => {
            inspected_sections(snapshot, target, inventory_page_start, inventory_page_size)
        }
        WorkflowPhase::Confirming => confirmation_sections(snapshot),
        WorkflowPhase::Revalidating => revalidation_sections(snapshot, spinner),
        WorkflowPhase::Committing => committing_sections(spinner),
    };
    ResetDetailDocument {
        title: heading("← Reset credit"),
        account: Some(StyledText {
            content: format!("{}  [{}]", target.account_label, target.account_tag),
            color: Color::White,
            weight: Weight::Normal,
        }),
        sections,
    }
}

fn inspection_sections(snapshot: &ResetWorkflowSnapshot, spinner: &str) -> Vec<ResetDetailSection> {
    let activities = snapshot.activities();
    vec![ResetDetailSection {
        role: ResetSectionRole::Content,
        title: heading("Checking live eligibility"),
        rows: vec![
            activity_field("Weekly usage", &activities.inspection_live_usage, spinner),
            activity_field(
                "Reset credits",
                &activities.inspection_credit_inventory,
                spinner,
            ),
            ResetDetailRow::Spacer,
            muted_text("No reset can be consumed while live data is loading."),
        ],
    }]
}

fn inspected_sections(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    inventory_page_start: usize,
    inventory_page_size: usize,
) -> Vec<ResetDetailSection> {
    let inventory = snapshot.credit_inventory();
    let usable_count = inventory
        .iter()
        .filter(|credit| credit.status == ResetCreditDisplayStatusDto::Available)
        .count();
    let inspection_eligible = snapshot.disabled_yes_reason().is_none() && usable_count > 0;
    let mut eligibility_rows = Vec::new();
    if let Some(weekly) = snapshot.live_weekly() {
        eligibility_rows.push(ResetDetailRow::Field {
            label: "Weekly remaining".to_owned(),
            value: format!(
                "{}% · {}",
                weekly.remaining_percent,
                if inspection_eligible {
                    "eligible"
                } else {
                    "not eligible"
                }
            ),
            value_color: if inspection_eligible {
                Color::Green
            } else {
                codex_router_tui::UiTextRole::Warning.color()
            },
        });
        if weekly.provenance == ResetValueProvenance::PreviousLiveRefreshing {
            eligibility_rows.push(muted_text("Previous live value shown while refreshing."));
        }
    }
    eligibility_rows.push(ResetDetailRow::Field {
        label: "Usable credits".to_owned(),
        value: usable_count.to_string(),
        value_color: Color::White,
    });
    if let Some(reason) = snapshot.disabled_yes_reason() {
        eligibility_rows.push(muted_text(disabled_reason(reason)));
    }

    vec![
        ResetDetailSection {
            role: ResetSectionRole::Content,
            title: heading("Live eligibility"),
            rows: eligibility_rows,
        },
        ResetDetailSection {
            role: ResetSectionRole::Inventory,
            title: heading(credit_section_title(
                inventory_page_start,
                inventory_page_size,
                inventory.len(),
                snapshot.credit_inventory_provenance(),
            )),
            rows: credit_rows(snapshot, target, inventory_page_start, inventory_page_size),
        },
    ]
}

fn credit_section_title(
    page_start: usize,
    page_size: usize,
    credit_count: usize,
    provenance: Option<ResetValueProvenance>,
) -> String {
    if credit_count == 0 {
        return "Reset credits".to_owned();
    }
    let page_end = page_start.saturating_add(page_size).min(credit_count);
    let source = match provenance {
        Some(ResetValueProvenance::CurrentLive) | None => "live",
        Some(ResetValueProvenance::PreviousLiveRefreshing) => "previous live · refreshing",
    };
    format!(
        "Reset credits · {source} · {}-{} of {credit_count}",
        page_start.saturating_add(1).min(credit_count),
        page_end
    )
}

fn credit_rows(
    snapshot: &ResetWorkflowSnapshot,
    target: &ResetPaneTarget,
    page_start: usize,
    page_size: usize,
) -> Vec<ResetDetailRow> {
    let inventory = snapshot.credit_inventory();
    if inventory.is_empty() {
        return vec![muted_text("None available")];
    }
    let page_end = page_start.saturating_add(page_size).min(inventory.len());
    let mut rows = inventory
        .iter()
        .skip(page_start)
        .take(page_size)
        .map(|credit| ResetDetailRow::Credit {
            marker: if credit.earliest_usable {
                "◆ next".to_owned()
            } else {
                String::new()
            },
            id_hint: format!("[{}]", credit.id_hint),
            description: format!(
                "{} · {} · {}",
                credit.title.as_deref().unwrap_or("Untitled reset credit"),
                credit_status_label(credit.status),
                format_credit_expiry(credit.expires_unix_seconds)
            ),
            marker_color: Color::Green,
        })
        .collect::<Vec<_>>();
    let remaining = inventory.len().saturating_sub(page_end);
    if remaining > 0 {
        rows.push(muted_text(format!("{remaining} more credits below")));
    }
    if saved_credit_count(&target.saved_reset_credits)
        .is_some_and(|saved_count| saved_count != inventory.len())
    {
        rows.push(ResetDetailRow::Spacer);
        rows.push(warning_text(format!(
            "Saved count differs: {} saved, {} live.",
            target.saved_reset_credits,
            inventory.len()
        )));
    }
    rows
}

fn confirmation_sections(snapshot: &ResetWorkflowSnapshot) -> Vec<ResetDetailSection> {
    let mut rows = Vec::new();
    if let Some(weekly) = snapshot.live_weekly() {
        rows.push(ResetDetailRow::Field {
            label: "Weekly remaining".to_owned(),
            value: format!("{}%", weekly.remaining_percent),
            value_color: Color::White,
        });
    }
    if let Some(credit) = snapshot.selected_credit() {
        rows.push(ResetDetailRow::Field {
            label: "Credit".to_owned(),
            value: format!(
                "[{}] {}",
                credit.id_hint,
                credit.title.as_deref().unwrap_or("Untitled reset credit")
            ),
            value_color: Color::White,
        });
        rows.push(ResetDetailRow::Field {
            label: "Expires".to_owned(),
            value: format_credit_expiry_value(credit.expires_unix_seconds),
            value_color: Color::White,
        });
    }
    rows.push(ResetDetailRow::Spacer);
    rows.push(ResetDetailRow::Choices {
        no_label: StyledText {
            content: if snapshot.confirmation_selection() == ConfirmationSelection::No {
                "[No]".to_owned()
            } else {
                "No".to_owned()
            },
            color: if snapshot.confirmation_selection() == ConfirmationSelection::No {
                Color::Yellow
            } else {
                Color::White
            },
            weight: Weight::Bold,
        },
        yes_label: StyledText {
            content: if snapshot.yes_enabled()
                && snapshot.confirmation_selection() == ConfirmationSelection::Yes
            {
                "[Yes]".to_owned()
            } else if snapshot.yes_enabled() {
                "Yes".to_owned()
            } else {
                "Yes disabled".to_owned()
            },
            color: if !snapshot.yes_enabled() {
                Color::Grey
            } else if snapshot.confirmation_selection() == ConfirmationSelection::Yes {
                Color::Yellow
            } else {
                Color::White
            },
            weight: Weight::Bold,
        },
    });
    if let Some(reason) = snapshot.disabled_yes_reason() {
        rows.push(muted_text(disabled_reason(reason)));
    }
    vec![ResetDetailSection {
        role: ResetSectionRole::Decision,
        title: heading("Confirm reset credit"),
        rows,
    }]
}

fn revalidation_sections(
    snapshot: &ResetWorkflowSnapshot,
    spinner: &str,
) -> Vec<ResetDetailSection> {
    let activities = snapshot.activities();
    vec![ResetDetailSection {
        role: ResetSectionRole::Content,
        title: heading("Rechecking live eligibility"),
        rows: vec![
            activity_field("Weekly usage", &activities.revalidation_live_usage, spinner),
            activity_field(
                "Reset credit",
                &activities.revalidation_credit_inventory,
                spinner,
            ),
            ResetDetailRow::Spacer,
            muted_text("The consume request has not been sent yet."),
        ],
    }]
}

fn committing_sections(spinner: &str) -> Vec<ResetDetailSection> {
    vec![ResetDetailSection {
        role: ResetSectionRole::Content,
        title: heading("Reset request sent"),
        rows: vec![
            ResetDetailRow::Field {
                label: "Provider".to_owned(),
                value: format!("{spinner} waiting for a definitive result"),
                value_color: codex_router_tui::UiTextRole::Secondary.color(),
            },
            ResetDetailRow::Spacer,
            muted_text("The request will not be retried automatically."),
        ],
    }]
}

fn result_document(result: Option<&WorkflowResult>) -> ResetDetailDocument {
    let (title, summary, assurance, color) = match result {
        Some(WorkflowResult::Known(KnownConsumeOutcome::Reset { windows_reset })) => (
            "Success — reset completed",
            format!("Provider confirmed {windows_reset} quota {} reset.",if *windows_reset == 1 {"window"}else {"windows"}),
            "One reset credit was consumed.".to_owned(),
            Color::Green,
        ),
        Some(WorkflowResult::Known(KnownConsumeOutcome::NothingToReset)) => (
            "Definitive provider result",
            "Provider reports nothing to reset.".to_owned(),
            "The provider returned a definitive response.".to_owned(),
            codex_router_tui::UiTextRole::Warning.color(),
        ),
        Some(WorkflowResult::Known(KnownConsumeOutcome::NoCredit)) => (
            "Definitive provider result",
            "Provider reports no reset credit.".to_owned(),
            "The provider returned a definitive response.".to_owned(),
            codex_router_tui::UiTextRole::Warning.color(),
        ),
        Some(WorkflowResult::Known(KnownConsumeOutcome::AlreadyRedeemed)) => (
            "Definitive provider result",
            "Provider reports credit already redeemed.".to_owned(),
            "The provider returned a definitive response.".to_owned(),
            codex_router_tui::UiTextRole::Warning.color(),
        ),
        Some(WorkflowResult::OutcomeUnknown(reason)) => (
            "Outcome unknown — do not retry",
            format!("No definitive response: {}.", reason.message()),
            "The credit may have been consumed. Refresh live credits before deciding what to do next.".to_owned(),
            Color::Red,
        ),
        Some(WorkflowResult::Refused(reason)) => (
            "Not consumed",
            format!("Reset refused before consume: {}.", reason.message()),
            "No consume request was sent. No reset credit was consumed.".to_owned(),
            codex_router_tui::UiTextRole::Warning.color(),
        ),
        None => (
            "Result unavailable",
            "Reset result unavailable.".to_owned(),
            "Do not retry until live credits have been inspected again.".to_owned(),
            Color::Red,
        ),
    };
    ResetDetailDocument {
        title: StyledText {
            content: title.to_owned(),
            color,
            weight: Weight::Bold,
        },
        account: None,
        sections: vec![ResetDetailSection {
            role: ResetSectionRole::Result,
            title: heading("Result"),
            rows: vec![
                normal(summary),
                StyledText {
                    content: assurance,
                    color,
                    weight: Weight::Normal,
                }
                .into(),
                ResetDetailRow::Spacer,
                muted_text(
                    "Saved quota may remain stale until the normal quota refresh updates it.",
                ),
                muted_text("Return to quota status."),
            ],
        }],
    }
}

impl From<StyledText> for ResetDetailRow {
    fn from(value: StyledText) -> Self {
        Self::Text(value)
    }
}

pub(super) fn reset_footer(snapshot: Option<&ResetWorkflowSnapshot>) -> &'static str {
    match snapshot.map(ResetWorkflowSnapshot::phase) {
        None | Some(WorkflowPhase::Browse) => {
            "↑/↓ focus  ctrl-r account options  esc/q exit  ctrl-c exit"
        }
        Some(WorkflowPhase::Inspecting) => "esc/ctrl-r back  ctrl-c exit without consume",
        Some(WorkflowPhase::Inspected) => {
            "←/esc back  enter review  pgup/pgdn credits  ctrl-c exit without consume"
        }
        Some(WorkflowPhase::Confirming) => {
            "←/→ select  enter confirm  esc/ctrl-r cancel  ctrl-c exit without consume"
        }
        Some(WorkflowPhase::Revalidating) => "esc/ctrl-r cancel before consume",
        Some(WorkflowPhase::Committing) => "waiting for definitive provider result",
        Some(WorkflowPhase::Result) => "enter/esc/ctrl-r back to quota browse",
    }
}
