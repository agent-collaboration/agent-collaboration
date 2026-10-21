use codex_router_tui::{UiHeading, UiHeadingLevel};
use iocraft::prelude::*;

use crate::quota::CreditUsageFreshness;
use crate::quota::CreditUsageStatus;
use crate::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot;
use crate::quota_reset::reset_session_supervisor::WorkflowPhase;
use codex_router_core::credit_usage::CreditAvailability;

use super::super::quota_reset_detail_rendering::render_reset_panel_content;
use super::super::quota_reset_detail_rendering::reset_options_content_height;
use super::super::quota_reset_presentation_model::ResetPaneTarget;
use super::super::quota_reset_presentation_model::reset_inventory_page_size;
use super::AccountOptionsMessage;
use super::AccountOptionsState;
use super::AccountOptionsTab;
use super::CreditPolicyEditorPhase;

pub(in crate::presentation::quota) struct AccountOptionsPanelProps<'a> {
    pub(in crate::presentation::quota) options: &'a AccountOptionsState,
    pub(in crate::presentation::quota) reset_snapshot: Option<&'a ResetWorkflowSnapshot>,
    pub(in crate::presentation::quota) reset_target: Option<&'a ResetPaneTarget>,
    pub(in crate::presentation::quota) width: usize,
    pub(in crate::presentation::quota) height: usize,
    pub(in crate::presentation::quota) inventory_page_start: usize,
    pub(in crate::presentation::quota) spinner_tick: usize,
}

pub(in crate::presentation::quota) fn render_account_options_panel(
    mut props: AccountOptionsPanelProps<'_>,
) -> AnyElement<'static> {
    let width = props.width;
    let height = props.height;
    props.width = width.saturating_sub(4);
    let identity = format!(
        "{} [{}]",
        props.options.target.account_label, props.options.target.account_tag
    );
    let editor_title = props.options.editor.is_some().then(
        || element!(UiHeading(content:"Credit usage".to_owned(),level:UiHeadingLevel::Section)),
    );
    let content = render_account_options_content(props, None);
    element! {View(width:width as u32,height:height as u32,flex_direction:FlexDirection::Column,border_style:BorderStyle::Single,border_color:Color::DarkGrey,padding_left:1,padding_right:1,overflow:Overflow::Hidden) {#(editor_title) Text(content:identity,color:Color::White) View(height:1) {} #(content)}}.into_any()
}

pub(in crate::presentation::quota) fn render_account_options_content(
    props: AccountOptionsPanelProps<'_>,
    state: Option<State<Option<AccountOptionsState>>>,
) -> AnyElement<'static> {
    let width = props.width.max(1);
    let height = props.height.max(1);
    let body_height = height.saturating_sub(7 + usize::from(props.options.pending_tab.is_some()));
    let body = match props.options.tab {
        AccountOptionsTab::Resets => render_resets_tab_body(
            props.options,
            props.reset_snapshot,
            props.reset_target,
            width.saturating_sub(2),
            body_height,
            props.inventory_page_start,
            props.spinner_tick,
        ),
        AccountOptionsTab::Credits => Some(render_credit_tab_body(
            props.options,
            width.saturating_sub(2),
            state,
        )),
    };
    let has_body = body.is_some();
    // Interactive dialogs already name the active page in their title and
    // expose tab navigation in their actions. Keep tabs in static panels.
    let show_tabs = state.is_none() && props.options.editor.is_none();
    let resets_tab = render_tab_label("Resets", props.options.tab == AccountOptionsTab::Resets);
    let credits_tab = render_tab_label("Credits", props.options.tab == AccountOptionsTab::Credits);

    element! {
        View(
            width: width as u32,
            flex_direction: FlexDirection::Column,
            flex_shrink:0.0_f32,max_width:100pct,
        ) {
            #(show_tabs.then(||element! {View(width:100pct,flex_direction:FlexDirection::Row,column_gap:2) {#(resets_tab) #(credits_tab)}}))
            #(props.options.pending_tab.is_some().then(|| element!(codex_router_tui::FocusReveal(has_focus:true) {Text(content: "Tab change pending", color: Color::Grey, wrap: TextWrap::Wrap)}).into_any()))
            #((show_tabs&&has_body).then(||element!(View(width:100pct,height:1,border_style:BorderStyle::Single,border_edges:Edges::Bottom,border_color:Color::DarkGrey))))
            #((show_tabs&&has_body).then(||element!(View(height:1))))
            #(body)
        }
    }
    .into_any()
}

pub(in crate::presentation::quota) fn account_options_content_height(
    options: &AccountOptionsState,
    reset_snapshot: Option<&ResetWorkflowSnapshot>,
    reset_target: Option<&ResetPaneTarget>,
    inventory_page_start: usize,
) -> usize {
    let options_header_rows = 7 + usize::from(options.pending_tab.is_some());
    if options.tab == AccountOptionsTab::Resets
        && let (Some(snapshot), Some(target)) = (reset_snapshot, reset_target)
        && snapshot.phase() != WorkflowPhase::Browse
    {
        return reset_options_content_height(snapshot, target, inventory_page_start)
            .saturating_add(options_header_rows);
    }
    match options.tab {
        AccountOptionsTab::Resets => {
            options_header_rows + 3 + usize::from(options.message.is_some())
        }
        AccountOptionsTab::Credits => {
            let base_body_rows = 9;
            let floor_row = usize::from(options.target.weekly_quota_floor_percent > 0);
            let provider_control_row =
                usize::from(provider_control_note(&options.target.credit_usage).is_some());
            let editor_status_row =
                usize::from(options.editor.as_ref().is_some_and(|editor| {
                    !matches!(editor.phase, CreditPolicyEditorPhase::Editing)
                }));
            options_header_rows
                + base_body_rows
                + floor_row
                + provider_control_row
                + editor_status_row
                + usize::from(options.message.is_some())
        }
    }
}

pub(in crate::presentation::quota) fn account_options_reset_inventory_page_size(
    body_height: usize,
) -> usize {
    // The options body omits the detail title, account line and detail-panel borders.
    const OMITTED_RESET_DETAIL_ROWS: usize = 4;
    reset_inventory_page_size(body_height.saturating_add(OMITTED_RESET_DETAIL_ROWS))
}

pub(super) fn account_options_message_text(message: AccountOptionsMessage) -> &'static str {
    match message {
        AccountOptionsMessage::Refreshing => "Refreshing credit balance…",
        AccountOptionsMessage::RefreshFailed => {
            "Credit refresh failed; cached observation retained."
        }
        AccountOptionsMessage::RefreshUnavailable(reason) => reason.message(),
        AccountOptionsMessage::Refreshed => "Credit balance refreshed.",
        AccountOptionsMessage::ResetReviewActive => {
            "Finish or cancel the reset review before switching tabs."
        }
    }
}

fn account_options_message_role(message: AccountOptionsMessage) -> codex_router_tui::UiTextRole {
    use codex_router_tui::UiTextRole;
    match message {
        AccountOptionsMessage::Refreshing => UiTextRole::Secondary,
        AccountOptionsMessage::Refreshed => UiTextRole::Success,
        AccountOptionsMessage::RefreshFailed => UiTextRole::Error,
        AccountOptionsMessage::RefreshUnavailable(_) | AccountOptionsMessage::ResetReviewActive => {
            UiTextRole::Warning
        }
    }
}

pub(in crate::presentation::quota) fn account_options_inspection_footer(
    phase: WorkflowPhase,
    width: usize,
) -> Option<&'static str> {
    const COMPACT_FOOTER_WIDTH: usize = 72;
    match phase {
        WorkflowPhase::Inspecting if width < COMPACT_FOOTER_WIDTH => {
            Some("tab/←/→ credits  esc/ctrl-r back")
        }
        WorkflowPhase::Inspecting => {
            Some("tab/←/→ credits  esc/ctrl-r back  ctrl-c exit without consume")
        }
        WorkflowPhase::Inspected if width < COMPACT_FOOTER_WIDTH => {
            Some("tab/←/→ credits enter review esc/ctrl-r back")
        }
        WorkflowPhase::Inspected => Some(
            "tab/←/→ credits  enter review  pgup/pgdn pages  esc/ctrl-r back  ctrl-c exit without consume",
        ),
        WorkflowPhase::Browse
        | WorkflowPhase::Confirming
        | WorkflowPhase::Revalidating
        | WorkflowPhase::Committing
        | WorkflowPhase::Result => None,
    }
}

pub(super) struct CreditUsageBodyProps<'a> {
    usage: &'a CreditUsageStatus,
    weekly_quota_floor_percent: u16,
    width: usize,
    editor: Option<&'a super::CreditPolicyEditorState>,
    message: Option<AccountOptionsMessage>,
    pending_tab: Option<super::AccountOptionsTab>,
    on_select: Handler<codex_router_core::credit_usage::CreditUsagePolicy>,
    on_open: Handler<()>,
}

pub(super) fn render_credit_usage_body(props: CreditUsageBodyProps<'_>) -> AnyElement<'static> {
    let CreditUsageBodyProps {
        usage,
        weekly_quota_floor_percent,
        width,
        editor,
        message,
        pending_tab,
        on_select,
        on_open,
    } = props;
    let entitlement = credit_entitlement_label(usage);
    let freshness = credit_freshness_label(usage.freshness);
    let age = if usage.age_label == "unknown" {
        "not checked".to_owned()
    } else {
        format!("checked {} ago", usage.age_label)
    };
    let policy = match editor {
        Some(editor) => editor.draft_policy,
        None => usage.policy,
    };
    let is_editing = editor.is_some_and(|editor| {
        matches!(
            editor.phase,
            CreditPolicyEditorPhase::Editing | CreditPolicyEditorPhase::SaveFailed(_)
        )
    });
    let form = editor.is_some();
    let balance_heading = (!form).then(|| {
        element! {
            UiHeading(content: "Credit balance".to_owned(), level: UiHeadingLevel::Section)
        }
        .into_any()
    });
    let usage_heading = (!form).then(|| {
        element! {
            UiHeading(content: "Credit usage".to_owned(), level: UiHeadingLevel::Section)
        }
        .into_any()
    });
    let policy_row = render_policy_row(CreditPolicyRowProps {
        policy,
        editing: is_editing,
        has_focus: pending_tab.is_none() && message.is_none(),
        editor,
        width,
        on_select,
        on_open,
    });
    let provider_note = provider_control_note(usage);
    let floor_note = (weekly_quota_floor_percent > 0)
        .then(|| format!("Weekly floor {weekly_quota_floor_percent}% blocks credit routing."));
    let editor_status = editor.and_then(|editor| match editor.phase {
        CreditPolicyEditorPhase::Editing => None,
        CreditPolicyEditorPhase::Saving => Some((
            "Saving preference…",
            codex_router_tui::UiTextRole::Secondary,
        )),
        CreditPolicyEditorPhase::SaveFailed(error) => {
            Some((error.message(), codex_router_tui::UiTextRole::Error))
        }
        CreditPolicyEditorPhase::SavedReloadFailed => Some((
            "Saved, but account data could not be reloaded.",
            codex_router_tui::UiTextRole::Warning,
        )),
    });
    let message = message.map(|message| {
        (
            account_options_message_text(message),
            account_options_message_role(message),
        )
    });
    let mut notices = Vec::new();
    for note in [floor_note, provider_note].into_iter().flatten() {
        notices.push(element!(codex_router_tui::UiText(content:format!("Warning: {note}"),role:codex_router_tui::UiTextRole::Warning,wrap:TextWrap::Wrap)).into_any());
    }
    if let Some((text, role)) = editor_status {
        let text = if role == codex_router_tui::UiTextRole::Error {
            format!("Error: {text}")
        } else if role == codex_router_tui::UiTextRole::Warning {
            format!("Warning: {text}")
        } else {
            text.to_owned()
        };
        notices.push(
            element!(codex_router_tui::FocusReveal(has_focus:pending_tab.is_none() && message.is_none()) {codex_router_tui::UiText(content:text,role,wrap:TextWrap::Wrap)}).into_any(),
        );
    }
    if let Some((text, role)) = message {
        let text = if role == codex_router_tui::UiTextRole::Warning {
            format!("Warning: {text}")
        } else {
            text.to_owned()
        };
        notices.push(element!(codex_router_tui::FocusReveal(has_focus:pending_tab.is_none()) {codex_router_tui::UiText(content:text,role,wrap:TextWrap::Wrap)}).into_any());
    }
    let has_notices = !notices.is_empty();

    element! {
        View(width: width as u32,max_width:100pct,flex_shrink:0.0_f32,flex_direction: FlexDirection::Column) {
            #(balance_heading)
            #(account_options_field_line(if form {"Balance"}else {"Status"}, &entitlement, width, Color::White))
            #((!form||usage.freshness!=crate::quota::CreditUsageFreshness::Fresh).then(||account_options_field_line("Freshness",freshness,width,freshness_color(usage.freshness))))
            #((!form).then(||account_options_field_line("Observation",&age,width,Color::Grey)))
            View(height: 1, flex_shrink: 0.0_f32) {}
            #(usage_heading)
            #(policy_row)
            #(has_notices.then(||element! {View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,margin_top:1,row_gap:1) {#(notices)}}))
        }
    }
    .into_any()
}

fn render_credit_tab_body(
    options: &AccountOptionsState,
    width: usize,
    state: Option<State<Option<AccountOptionsState>>>,
) -> AnyElement<'static> {
    let expected_account = options.target.account_id.clone();
    let open_account = expected_account.clone();
    let expected_session = options.session_generation;
    let expected_operation = options
        .editor
        .as_ref()
        .map(|editor| editor.operation_generation);
    let on_select = Handler::from(move |policy| {
        if let Some(mut state) = state {
            let mut current = state.write();
            if let Some(options) = current.as_mut().filter(|options| {
                options.target.account_id == expected_account
                    && options.session_generation == expected_session
            }) && let Some(editor) = options.editor.as_mut().filter(|editor| {
                Some(editor.operation_generation) == expected_operation
                    && matches!(
                        editor.phase,
                        CreditPolicyEditorPhase::Editing | CreditPolicyEditorPhase::SaveFailed(_)
                    )
            }) {
                editor.draft_policy = policy;
                editor.phase = CreditPolicyEditorPhase::Editing;
                editor.choice = codex_router_tui::SearchSelectState::Closed;
            }
        }
    });
    let on_open = Handler::from(move |_| {
        if let Some(mut state) = state {
            let mut current = state.write();
            if let Some(options) = current.as_mut().filter(|options| {
                options.target.account_id == open_account
                    && options.session_generation == expected_session
            }) && let Some(editor) = options.editor.as_mut().filter(|editor| {
                Some(editor.operation_generation) == expected_operation
                    && matches!(
                        editor.phase,
                        CreditPolicyEditorPhase::Editing | CreditPolicyEditorPhase::SaveFailed(_)
                    )
            }) {
                editor.choice.open(editor.draft_policy);
            }
        }
    });
    render_credit_usage_body(CreditUsageBodyProps {
        usage: &options.target.credit_usage,
        weekly_quota_floor_percent: options.target.weekly_quota_floor_percent,
        width,
        editor: options.editor.as_ref(),
        message: options.message,
        pending_tab: options.pending_tab,
        on_select,
        on_open,
    })
}

fn render_resets_tab_body(
    options: &AccountOptionsState,
    reset_snapshot: Option<&ResetWorkflowSnapshot>,
    reset_target: Option<&ResetPaneTarget>,
    width: usize,
    height: usize,
    inventory_page_start: usize,
    spinner_tick: usize,
) -> Option<AnyElement<'static>> {
    if let (Some(snapshot), Some(target)) = (reset_snapshot, reset_target)
        && snapshot.phase() != WorkflowPhase::Browse
    {
        return Some(render_reset_panel_content(
            snapshot,
            target,
            width,
            inventory_page_start,
            account_options_reset_inventory_page_size(height),
            spinner_tick,
            None,
        ));
    }
    let unavailable_reason = if !options.target.enabled {
        Some("Resets are unavailable while this account is disabled.")
    } else if options.target.credential_generation.is_none() {
        Some("Resets are unavailable without active credentials.")
    } else {
        None
    };
    let explanation =
        unavailable_reason.or_else(|| reset_target.map(|_| "Checking reset eligibility…"));
    if explanation.is_none() && options.message.is_none() {
        return Some(account_options_field_line(
            "Eligibility",
            "Not checked",
            width,
            Color::White,
        ));
    }
    Some(element! {
        View(width:width as u32,max_width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
            #(explanation.map(|text|element!(codex_router_tui::UiText(content:text.to_owned(),role:codex_router_tui::UiTextRole::Secondary,wrap:TextWrap::Wrap))))
            #(options.message.map(|message|element!(codex_router_tui::UiText(content:account_options_message_text(message).to_owned(),role:account_options_message_role(message),wrap:TextWrap::Wrap))))
        }
    }.into_any())
}

fn render_tab_label(label: &str, active: bool) -> AnyElement<'static> {
    let (content, color, weight) = if active {
        (format!("[ {label} ]"), Color::Yellow, Weight::Bold)
    } else {
        (label.to_owned(), Color::Grey, Weight::Normal)
    };
    element! { Text(content, color, weight, wrap: TextWrap::NoWrap) }.into_any()
}

struct CreditPolicyRowProps<'a> {
    policy: codex_router_core::credit_usage::CreditUsagePolicy,
    editing: bool,
    has_focus: bool,
    editor: Option<&'a super::CreditPolicyEditorState>,
    width: usize,
    on_select: Handler<codex_router_core::credit_usage::CreditUsagePolicy>,
    on_open: Handler<()>,
}
fn render_policy_row(props: CreditPolicyRowProps<'_>) -> AnyElement<'static> {
    let CreditPolicyRowProps {
        policy,
        editing,
        has_focus,
        editor,
        width,
        on_select,
        on_open,
    } = props;
    let label = if policy == codex_router_core::credit_usage::CreditUsagePolicy::Allow {
        "Allow"
    } else {
        "Disallow"
    };
    if !editing {
        return element!(codex_router_tui::FocusReveal(has_focus:editor.is_none()&&has_focus) {#(account_options_field_line("Use credits",label,width,Color::White))}).into_any();
    }
    let choice = editor
        .map(|editor| editor.choice.clone())
        .unwrap_or_default();
    let expanded = choice.is_open();
    let body = if expanded {
        element!(codex_router_tui::SearchSelect<codex_router_core::credit_usage::CreditUsagePolicy>(state:choice,options:super::credit_policy_choices(),on_select)).into_any()
    } else {
        element! {codex_router_tui::DialogButton(handler:on_open) {Text(content:format!("{label} ▾"),color:if has_focus {Color::Yellow}else {Color::White})}}
            .into_any()
    };
    let body = if expanded {
        body
    } else {
        element!(codex_router_tui::SelectableRow(selected:false) {#(body)}).into_any()
    };
    element! {codex_router_tui::FormField(label:"Use credits".to_owned(),label_width:if width<52 {11u16}else {14u16},stacked:width<34,has_focus:has_focus&&editor.is_some_and(|editor|editor.phase==CreditPolicyEditorPhase::Editing)) {#(body)}}.into_any()
}

fn account_options_field_line(
    label: &str,
    value: &str,
    width: usize,
    value_color: Color,
) -> AnyElement<'static> {
    element! {
        View(width:width as u32,max_width:100pct,flex_shrink:0.0_f32) {
            codex_router_tui::FormField(label:label.to_owned(),label_width:14u16,stacked:width<52) {Text(content:value.to_owned(),color:value_color)}
        }
    }.into_any()
}

fn credit_entitlement_label(usage: &CreditUsageStatus) -> String {
    match usage.provider_observation.availability() {
        CreditAvailability::Unknown => "Unknown".to_owned(),
        CreditAvailability::Depleted => "Depleted".to_owned(),
        CreditAvailability::Unlimited => "Unlimited".to_owned(),
        CreditAvailability::Available {
            balance: Some(balance),
        } => format!(
            "Available · {}",
            super::credit_balance_presentation::detailed_credit_balance(balance)
        ),
        CreditAvailability::Available { balance: None } => {
            "Available · balance withheld".to_owned()
        }
    }
}

fn credit_freshness_label(freshness: CreditUsageFreshness) -> &'static str {
    match freshness {
        CreditUsageFreshness::Fresh => "Fresh",
        CreditUsageFreshness::Stale => "Stale",
        CreditUsageFreshness::Unknown => "Unknown",
    }
}

fn freshness_color(freshness: CreditUsageFreshness) -> Color {
    match freshness {
        CreditUsageFreshness::Fresh => Color::Green,
        CreditUsageFreshness::Stale => codex_router_tui::UiTextRole::Warning.color(),
        CreditUsageFreshness::Unknown => Color::Grey,
    }
}

fn provider_control_note(usage: &CreditUsageStatus) -> Option<String> {
    use codex_router_core::credit_usage::CreditSpendControl;

    match usage.provider_observation.spend_control() {
        CreditSpendControl::Reached => Some("Provider spend control is reached.".to_owned()),
        CreditSpendControl::Unknown => Some("Provider spend-control state is unknown.".to_owned()),
        CreditSpendControl::Clear | CreditSpendControl::Unreported => usage
            .provider_observation
            .limit_reason()
            .filter(|reason| reason.blocks_credit_usage())
            .map(|reason| format!("Provider reports {}.", reason.as_str().replace('_', " "))),
    }
}
