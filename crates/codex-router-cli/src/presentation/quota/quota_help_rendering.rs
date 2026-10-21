//! Quota-owned contextual Help and surface chrome; no command or runtime ownership.
use codex_router_tui::{
    DialogAction, DialogActionEmphasis, ShortcutEntry, ShortcutHelp, ShortcutSection,
    SurfaceHeader, UiText, UiTextRole,
};
use iocraft::prelude::*;

use super::quota_status_view_model::QuotaStatusViewModel;
use super::responsive_quota_layout::quota_title_row;

#[derive(Clone, Copy, Default)]
pub(super) enum QuotaHelpMode {
    #[default]
    Browse,
    Filters,
    Details,
    Floor,
    Options,
    Reset,
}

pub(super) fn quota_help_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::F(1) | KeyCode::Char('\u{1f}'))
        || (matches!(code, KeyCode::Char('/' | '_' | '7'))
            && modifiers.contains(KeyModifiers::CONTROL))
}

pub(super) fn quota_help_action(mut open: State<bool>) -> AnyElement<'static> {
    element!(DialogAction(label: "Help".to_owned(), emphasis: DialogActionEmphasis::Secondary, handler: move |_| open.set(true))).into_any()
}

pub(super) struct QuotaHeaderProps<'a> {
    pub model: &'a QuotaStatusViewModel,
    pub width: usize,
    pub tick: usize,
    pub interactive: bool,
    pub inactive: bool,
    pub help: State<bool>,
    pub details_action: AnyElement<'static>,
}

pub(super) fn render_quota_header(props: QuotaHeaderProps<'_>) -> AnyElement<'static> {
    let QuotaHeaderProps {
        model,
        width,
        tick,
        interactive,
        inactive,
        help,
        details_action,
    } = props;
    if !interactive {
        return quota_title_row(model, width, tick);
    }
    let column_gap: u16 = if width < 64 { 1 } else { 2 };
    let status_width = width.saturating_sub(23 + 3 * usize::from(column_gap));
    let status = model
        .header
        .freshness
        .summary(model.header.observed_clients);
    let show_status = width >= 64;
    let separate =
        show_status && unicode_width::UnicodeWidthStr::width(status.as_str()) > status_width;
    let children = if separate || !show_status {
        vec![details_action]
    } else {
        vec![
            element!(UiText(content:status,role:UiTextRole::Secondary)).into_any(),
            details_action,
        ]
    };
    element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        SurfaceHeader(title:"Quota status".to_owned(),inactive,column_gap,help_action:Some(quota_help_action(help)),children)
        #(separate.then(||element!(Text(content:model.header.freshness.summary(model.header.observed_clients),color:Color::Grey,wrap:TextWrap::Wrap))))
    }}.into_any()
}

pub(super) fn render_quota_route(
    model: &QuotaStatusViewModel,
    width: usize,
) -> AnyElement<'static> {
    let route = format!("Pool-wide: {}", model.header.route_band);
    let counts = model.header.pool.summary(width);
    let data_problems = [
        ("Stale", model.header.freshness.stale),
        ("Data unknown", model.header.freshness.unknown),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(label, count)| format!("{label} {count}"))
    .collect::<Vec<_>>()
    .join(" · ");
    element! {View(width:100pct,column_gap:2,flex_wrap:FlexWrap::Wrap,flex_shrink:0.0_f32) {
        Text(content:route,color:Color::White,wrap:TextWrap::Wrap)
        View(max_width:100pct,flex_shrink:0.0_f32) {Text(content:counts,color:Color::Grey,wrap:TextWrap::Wrap)}
        #((width<64 && !data_problems.is_empty()).then(||element!(Text(content:data_problems,color:Color::Grey,wrap:TextWrap::Wrap))))
        #(model.selection_projection_degraded.then(||element!(Text(content:"degraded",color:Color::White))))
        #(model.header.credential_notice.as_ref().map(|notice|element!(Text(content:notice.clone(),color:Color::White,wrap:TextWrap::Wrap))))
        #((model.header.pool.total()==0).then(||element!(Text(content:"no accounts",color:Color::White))))
    }}.into_any()
}

pub(super) fn render_quota_help(
    width: u16,
    height: u16,
    mode: QuotaHelpMode,
    mut open: State<bool>,
    header: &super::quota_status_symbols::QuotaHeaderFacts,
    identity: Option<&super::quota_dialog_context::QuotaDialogIdentity>,
) -> AnyElement<'static> {
    let (title, entries) = match mode {
        QuotaHelpMode::Browse => (
            "Browse accounts",
            vec![
                ("↑ / ↓", "Focus account"),
                ("PgUp / PgDn", "Scroll account content"),
                ("Ctrl+F / F3", "Open filters"),
                ("Enter / F4", "Account details"),
                ("Ctrl+E", "Edit weekly floor"),
                ("Ctrl+R", "Account options"),
                ("Esc / Q", "Exit"),
            ],
        ),
        QuotaHelpMode::Filters => (
            "Account filters",
            vec![
                ("↑ / ↓", "Move field or choice focus"),
                ("Tab / Shift+Tab", "Move focus"),
                ("Type / Paste", "Search an open choice"),
                ("Space", "Toggle disabled accounts"),
                ("Enter", "Open choice; Apply draft when focused"),
                ("Esc", "Close choice; otherwise discard draft"),
            ],
        ),
        QuotaHelpMode::Details => (
            "Account details",
            vec![
                ("↑ / ↓ / PgUp / PgDn", "Scroll details"),
                ("Esc", "Close details"),
            ],
        ),
        QuotaHelpMode::Floor => (
            "Weekly floor",
            vec![
                ("← / →", "Adjust floor"),
                ("Enter", "Save or retry refresh"),
                ("Esc / Ctrl+E", "Cancel uncommitted edit"),
            ],
        ),
        QuotaHelpMode::Options => (
            "Account options",
            vec![
                ("Tab / ← / →", "Switch Resets and Credits"),
                ("Enter", "Inspect resets or edit/save credit preference"),
                ("R", "Refresh credit usage"),
                ("Esc / Ctrl+R", "Back"),
            ],
        ),
        QuotaHelpMode::Reset => (
            "Reset workflow",
            vec![
                ("PgUp / PgDn", "Page reset inventory"),
                ("Enter", "Review or confirm selected action"),
                ("← / →", "Select No or enabled Yes"),
                ("Esc", "Cancel before commitment"),
            ],
        ),
    };
    let mut sections = vec![
        ShortcutSection {
            title: "Current quota".to_owned(),
            entries: vec![
                ShortcutEntry {
                    keys: "Pool".to_owned(),
                    description: header.pool.summary(usize::MAX),
                },
                ShortcutEntry {
                    keys: "Data".to_owned(),
                    description: header.freshness.summary(header.observed_clients),
                },
            ],
        },
        ShortcutSection {
            title: title.to_owned(),
            entries: entries
                .into_iter()
                .map(|(keys, description)| ShortcutEntry {
                    keys: keys.to_owned(),
                    description: description.to_owned(),
                })
                .collect(),
        },
        ShortcutSection {
            title: "Quota legend".to_owned(),
            entries: [
                (
                    "Σ / ✓ / ◇ / ⊘ / ? / ×",
                    "Pool: accounts / usable / reserve / blocked / unknown / excluded",
                ),
                (
                    "● / ◷ / ?",
                    "Samples: fresh / stale / unknown; independent of availability",
                ),
                (
                    "★ / ✓ / ◇ / ⊘",
                    "Row: preferred next / available / held / blocked",
                ),
                (
                    "◷ / ?",
                    "Row: stale / unknown; credential notices override status",
                ),
                (
                    "⇄ / ↻",
                    "Observed serving clients / available reset credits",
                ),
                ("Weekly pace", "Forecast for the weekly quota window"),
            ]
            .into_iter()
            .map(|(keys, description)| ShortcutEntry {
                keys: keys.to_owned(),
                description: description.to_owned(),
            })
            .collect(),
        },
        ShortcutSection {
            title: "Help".to_owned(),
            entries: vec![
                ShortcutEntry {
                    keys: "F1 / Ctrl+/".to_owned(),
                    description: "Open Help; F1 or Esc closes Help".to_owned(),
                },
                ShortcutEntry {
                    keys: "Ctrl+C / Ctrl+D".to_owned(),
                    description: "Exit Quota unless a reset is being applied.".to_owned(),
                },
            ],
        },
    ];
    if matches!(mode, QuotaHelpMode::Options | QuotaHelpMode::Reset) {
        sections.insert(2,ShortcutSection {title:"Credit and reset guidance".to_owned(),entries:vec![
            ShortcutEntry {keys:"Included quota".to_owned(),description:"Included quota on eligible accounts always comes first.".to_owned()},
            ShortcutEntry {keys:"Allow credits".to_owned(),description:"Allow is limited to eligible OpenAI Responses requests.".to_owned()},
            ShortcutEntry {keys:"Inspect".to_owned(),description:"Inspect reset eligibility. No reset is consumed without explicit confirmation.".to_owned()},
        ]});
    }
    if let Some(identity) = identity {
        sections.insert(
            0,
            ShortcutSection {
                title: "Selected account".to_owned(),
                entries: vec![ShortcutEntry {
                    keys: "Account".to_owned(),
                    description: identity.full_label(),
                }],
            },
        );
    }
    element!(ShortcutHelp(width, height, title: "Quota Help".to_owned(), sections, actions: vec![element!(DialogAction(label: "Close".to_owned(), handler: move |_| open.set(false))).into_any()])).into_any()
}

pub(super) struct QuotaScreenChromeProps {
    pub width: usize,
    pub horizontal_padding: usize,
    pub height: Option<usize>,
    pub header: AnyElement<'static>,
    pub route: AnyElement<'static>,
    pub controls: Vec<AnyElement<'static>>,
    pub body: AnyElement<'static>,
    pub actions: Vec<AnyElement<'static>>,
    pub static_footer: Option<String>,
}

pub(super) fn render_quota_screen(props: QuotaScreenChromeProps) -> AnyElement<'static> {
    element! {
        View(width: props.width as u32, height: props.height.map_or(Size::Auto,|height|Size::Length(height as u32)), border_style: BorderStyle::Round, border_color: Color::Cyan, overflow: Overflow::Hidden, padding_left: props.horizontal_padding as u32, padding_right: props.horizontal_padding as u32, flex_direction: FlexDirection::Column) {
            #(props.header)
            #(props.route)
            #(props.controls)
            #(props.body)
            View(width: 100pct, flex_grow: 1.0_f32) {}
            View(flex_shrink: 0.0_f32) { #(props.actions) }
            #(props.static_footer.map(|line| element!(Text(content: line, color: Color::Grey, wrap: TextWrap::NoWrap)).into_any()))
        }
    }.into_any()
}

// Legacy static output keeps its current workflow footer; interactive Help owns
// shortcut presentation instead. This function has no input or effect handling.
pub(super) struct QuotaFooterPresentation<'a> {
    pub weekly_floor_editor: Option<&'a super::quota_floor_editor::WeeklyFloorEditorState>,
    pub account_options: Option<&'a super::quota_account_options::AccountOptionsState>,
    pub reset_snapshot:
        Option<&'a crate::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot>,
    pub width: usize,
    pub weekly_floor_editing_enabled: bool,
    pub reset_detail_active: bool,
    pub filters: super::quota_provider_filters::QuotaFilterState,
    pub filter_modal_owns_input: bool,
}

pub(super) fn quota_legacy_footer(props: QuotaFooterPresentation<'_>) -> String {
    use super::quota_account_options::account_options_footer;
    use super::quota_floor_editor::weekly_floor_editor_footer;
    use super::quota_reset_detail_rendering::reset_footer;
    let inspection_footer = props
        .account_options
        .and(props.reset_snapshot)
        .and_then(|snapshot| {
            if props
                .account_options
                .is_some_and(|options| options.pending_tab.is_some())
                && matches!(
                    snapshot.phase(),
                    crate::quota_reset::reset_session_supervisor::WorkflowPhase::Inspecting
                        | crate::quota_reset::reset_session_supervisor::WorkflowPhase::Inspected
                )
            {
                Some("tab change pending  esc back  ctrl-c exit")
            } else {
                super::quota_account_options::account_options_inspection_footer(
                    snapshot.phase(),
                    props.width,
                )
            }
        });
    let mut footer_line = props.weekly_floor_editor.map_or_else(
        || {
            props.account_options.map_or_else(
                || {
                    if props.weekly_floor_editing_enabled && !props.reset_detail_active {
                        "↑/↓ focus  ctrl-e edit floor  ctrl-r account options  esc/q exit  ctrl-c exit".to_owned()
                    } else {
                        reset_footer(props.reset_snapshot).to_owned()
                    }
                },
                |options| {
                    if props.reset_detail_active {
                        inspection_footer.map_or_else(
                            || reset_footer(props.reset_snapshot).to_owned(),
                            str::to_owned,
                        )
                    } else {
                        account_options_footer(options)
                    }
                },
            )
        },
        |editor| weekly_floor_editor_footer(editor).to_owned(),
    );
    if props.filters.draft.is_some() && !props.filter_modal_owns_input {
        footer_line = "↑/↓ provider  tab focus  space toggle  enter apply  esc cancel".to_owned();
    }
    footer_line
}
