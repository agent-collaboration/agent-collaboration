//! Quota-owned contextual Help and surface chrome; no command or runtime ownership.
use agent_collaboration_tui::{
    DialogAction, DialogActionEmphasis, ShortcutEntry, ShortcutHelp, ShortcutSection,
    SurfaceHeader, UiText, UiTextRole,
};
use iocraft::prelude::*;

use super::responsive_quota_layout::quota_title_row;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;

#[derive(Clone, Copy, Default)]
pub(crate) enum QuotaHelpMode {
    #[default]
    Browse,
    Filters,
    Details,
    PoolDetails,
    Floor,
    Options,
    Reset,
}

pub(crate) fn quota_help_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::F(1) | KeyCode::Char('\u{1f}'))
        || (matches!(code, KeyCode::Char('/' | '_' | '7'))
            && modifiers.contains(KeyModifiers::CONTROL))
}

pub(crate) fn quota_help_action(mut open: State<bool>) -> AnyElement<'static> {
    element!(DialogAction(label: "Help".to_owned(), emphasis: DialogActionEmphasis::Secondary, handler: move |_| open.set(true))).into_any()
}

pub(crate) struct QuotaHeaderProps<'a> {
    pub(crate) model: &'a QuotaStatusViewModel,
    pub(crate) width: usize,
    pub(crate) tick: usize,
    pub(crate) interactive: bool,
    pub(crate) inactive: bool,
    pub(crate) help: State<bool>,
    pub(crate) details_action: AnyElement<'static>,
}

pub(crate) fn render_quota_header(props: QuotaHeaderProps<'_>) -> AnyElement<'static> {
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
    let clients = model.header.observed_clients.map(|count| {
        element!(UiText(content:format!("Clients {count}"),role:UiTextRole::Secondary)).into_any()
    });
    let mut children = Vec::new();
    if width >= 64 {
        children.extend(clients);
    }
    children.push(details_action);
    element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        SurfaceHeader(title:"Quota status".to_owned(),inactive,column_gap,help_action:Some(quota_help_action(help)),children)
    }}.into_any()
}

pub(crate) fn render_quota_route(
    model: &QuotaStatusViewModel,
    width: usize,
) -> AnyElement<'static> {
    let _ = width;
    element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        #(model.selection_projection_degraded.then(||element!(UiText(content:"Selection projection degraded",role:UiTextRole::Error,wrap:TextWrap::Wrap))))
        #(model.header.credential_notice.as_ref().map(|notice|element!(UiText(content:notice.clone(),role:UiTextRole::Error,wrap:TextWrap::Wrap))))
    }}.into_any()
}

pub(crate) fn render_quota_help(
    width: u16,
    height: u16,
    mode: QuotaHelpMode,
    mut open: State<bool>,
    header: &codex_router_quota_service::quota_display_facts::QuotaHeaderFacts,
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
                ("F5", "Pool details"),
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
        QuotaHelpMode::Details | QuotaHelpMode::PoolDetails => (
            if matches!(mode, QuotaHelpMode::PoolDetails) {
                "Pool details"
            } else {
                "Account details"
            },
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

pub(crate) struct QuotaScreenChromeProps {
    pub(crate) width: usize,
    pub(crate) horizontal_padding: usize,
    pub(crate) height: Option<usize>,
    pub(crate) header: AnyElement<'static>,
    pub(crate) route: AnyElement<'static>,
    pub(crate) controls: Vec<AnyElement<'static>>,
    pub(crate) body: AnyElement<'static>,
    pub(crate) actions: Vec<AnyElement<'static>>,
    pub(crate) static_footer: Option<String>,
}

pub(crate) fn render_quota_screen(props: QuotaScreenChromeProps) -> AnyElement<'static> {
    element! {
        View(width: u32::try_from(props.width).unwrap_or(u32::MAX), height: props.height.map_or(Size::Auto,|height|Size::Length(u32::try_from(height).unwrap_or(u32::MAX))), border_style: BorderStyle::Round, border_color: Color::Cyan, overflow: Overflow::Hidden, padding_left: u32::try_from(props.horizontal_padding).unwrap_or(u32::MAX), padding_right: u32::try_from(props.horizontal_padding).unwrap_or(u32::MAX), flex_direction: FlexDirection::Column) {
            #(props.header)
            #(props.route)
            #(props.controls)
            #(props.body)
            View(width: 100pct, flex_grow: 1.0_f32) {}
            #((!props.actions.is_empty()).then(||element!(agent_collaboration_tui::DialogFooter(children:props.actions))))
            #(props.static_footer.map(|line| element!(Text(content: line, color: Color::Grey, wrap: TextWrap::NoWrap)).into_any()))
        }
    }.into_any()
}

// Legacy static output keeps its current workflow footer; interactive Help owns
// shortcut presentation instead. This function has no input or effect handling.
pub(crate) struct QuotaFooterPresentation<'a> {
    pub(crate) weekly_floor_editor: Option<&'a super::quota_floor_editor::WeeklyFloorEditorState>,
    pub(crate) account_options: Option<&'a super::quota_account_options::AccountOptionsState>,
    pub(crate) reset_snapshot:
        Option<&'a codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot>,
    pub(crate) width: usize,
    pub(crate) weekly_floor_editing_enabled: bool,
    pub(crate) reset_detail_active: bool,
    pub(crate) filters: super::quota_provider_filters::QuotaFilterState,
    pub(crate) filter_modal_owns_input: bool,
}

pub(crate) fn quota_legacy_footer(props: QuotaFooterPresentation<'_>) -> String {
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
                    codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspecting
                        | codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase::Inspected
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

/// Browse uses the same footer surface as dialogs; the controller owns activation.
pub(crate) fn render_quota_browse_footer(
    enabled: bool,
    weekly_floor_enabled: bool,
    on_details: impl FnMut(()) + Send + Sync + 'static,
) -> Vec<AnyElement<'static>> {
    let mut commands = vec![
        element!(DialogAction(label:"F4 Details",enabled,handler:on_details)).into_any(),
        element!(UiText(content:"F3 Filters",role:UiTextRole::Label,wrap:TextWrap::NoWrap))
            .into_any(),
        element!(UiText(content:"F5 Pool",role:UiTextRole::Label,wrap:TextWrap::NoWrap)).into_any(),
        element!(UiText(content:"F1 Help",role:UiTextRole::Label,wrap:TextWrap::NoWrap)).into_any(),
        element!(UiText(content:"Ctrl+R Options",role:UiTextRole::Label,wrap:TextWrap::NoWrap))
            .into_any(),
    ];
    if weekly_floor_enabled {
        commands.push(
            element!(UiText(content:"Ctrl+E Floor",role:UiTextRole::Label,wrap:TextWrap::NoWrap))
                .into_any(),
        );
    }
    commands
}
