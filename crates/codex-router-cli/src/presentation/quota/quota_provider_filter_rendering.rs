use codex_router_tui::UseDialogInteraction;
use iocraft::prelude::*;

use super::quota_provider_filters::*;

const NARROW_FILTER_TOOLBAR_WIDTH: usize = 90;

pub(super) const fn quota_filter_toolbar_height(_width: usize) -> usize {
    1
}

pub(super) fn quota_filter_controls_height(
    mode: QuotaInteractionMode,
    width: usize,
    _chooser_open: bool,
    modal_owns_input: bool,
) -> usize {
    if mode == QuotaInteractionMode::Static || modal_owns_input {
        return 0;
    }
    quota_filter_toolbar_height(width)
}

#[derive(Default, Props)]
struct QuotaFilterButtonProps {
    label: String,
    selected: bool,
    has_focus: bool,
    enabled: bool,
    on_click: HandlerMut<'static, ()>,
}

#[component]
fn QuotaFilterButton(
    props: &mut QuotaFilterButtonProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let effective_enabled = props.enabled && hooks.dialog_input_enabled();
    hooks.use_dialog_local_events({
        let enabled = props.enabled;
        let mut on_click = props.on_click.take();
        move |event| {
            if enabled
                && matches!(
                    event,
                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        ..
                    })
                )
            {
                on_click(());
            }
        }
    });
    element! {
        View(flex_shrink: 0.0_f32) {
            Text(content: props.label.clone(), color: if !effective_enabled { Color::DarkGrey } else if props.selected || props.has_focus { Color::Yellow } else { Color::White }, weight: if effective_enabled && (props.selected || props.has_focus) {Weight::Bold} else {Weight::Normal}, decoration: if effective_enabled && props.has_focus { TextDecoration::Underline } else { TextDecoration::None }, wrap: TextWrap::NoWrap)
        }
    }
}

pub(super) fn render_quota_filter_toolbar(
    filters: QuotaAccountFilters,
    visible_count: usize,
    loaded_count: usize,
    width: usize,
    enabled: bool,
    on_open: impl FnMut(()) + Send + Sync + 'static,
) -> AnyElement<'static> {
    let summary = quota_filter_summary(filters, visible_count, loaded_count);
    let narrow = width < NARROW_FILTER_TOOLBAR_WIDTH;
    let full_label = format!(
        "{} · Disabled: {} · {visible_count}/{loaded_count} ▾",
        filters.provider.label(),
        filters.disabled.label()
    );
    element! {
        View(width: width as u32, height: 1, column_gap: 2, flex_shrink: 0.0_f32) {
            QuotaFilterButton(label: if narrow { full_label } else { "Filters ▾".to_owned() }, enabled, on_click: on_open)
            #(if !narrow { Some(element!(Text(content: summary, color: Color::White, wrap: TextWrap::NoWrap))) } else { None })
        }
    }.into_any()
}

fn quota_filter_summary(
    filters: QuotaAccountFilters,
    visible_count: usize,
    loaded_count: usize,
) -> String {
    format!(
        "Provider: {} · Disabled: {} · {visible_count}/{loaded_count} accounts",
        filters.provider.label(),
        filters.disabled.label()
    )
}

pub(super) fn render_quota_filter_chooser(
    draft: QuotaFilterDraft,
    width: usize,
    controller: QuotaFilterController,
) -> AnyElement<'static> {
    let choice = controller.choice.read().clone();
    let fields=[(QuotaFilterFocus::Provider,"Account provider",draft.filters.provider.label()),(QuotaFilterFocus::Disabled,"Disabled accounts",draft.filters.disabled.label())].into_iter().map(|(field,label,value)| {
        let expanded=draft.focus==field&&choice.is_open();
        let body=if expanded {
            element!(codex_router_tui::SearchSelect<QuotaFilterValue>(state:choice.clone(),options:quota_filter_choices(field),on_select:move |value|controller.select_choice(value))).into_any()
        } else {
            element!(QuotaFilterButton(label:format!("{value} ▾"),selected:false,has_focus:draft.focus==field,enabled:true,on_click:move |_|controller.open_choice(field))).into_any()
        };
        let body=if expanded {body}else {element!(codex_router_tui::SelectableRow(selected:false) {#(body)}).into_any()};
        element! {codex_router_tui::FormField(label:label.to_owned(),label_width:17u16,stacked:width<52,has_focus:draft.focus==field) {#(body)}}.into_any()
    }).collect::<Vec<_>>();
    element! {
        View(width:width as u32,max_width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
            #(fields)
        }
    }.into_any()
}

pub(super) fn render_quota_details_button(
    enabled: bool,
    on_open: impl FnMut(()) + Send + Sync + 'static,
) -> AnyElement<'static> {
    element!(QuotaFilterButton(label: "Details".to_owned(), enabled, on_click: on_open)).into_any()
}

pub(super) fn render_quota_filter_actions(
    draft: QuotaFilterDraft,
    controller: QuotaFilterController,
) -> Vec<AnyElement<'static>> {
    let choice = controller.choice.read().clone();
    let expanded = choice.is_open();
    let enabled = !expanded
        || choice
            .available_focused_key(&quota_filter_choices(draft.focus))
            .is_some();
    vec![
        element!(codex_router_tui::DialogAction(label:"Cancel",keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,has_focus:draft.focus==QuotaFilterFocus::Cancel,emphasis:codex_router_tui::DialogActionEmphasis::Secondary,handler:move |_|controller.dispatch(QuotaFilterAction::Cancel))).into_any(),
        element!(codex_router_tui::DialogAction(label:if expanded {"Select"}else {"Apply"},enabled,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,has_focus:!expanded&&draft.focus==QuotaFilterFocus::Apply,handler:move |_| {
            if controller.choice.read().is_open() {controller.handle_key(KeyCode::Enter,KeyModifiers::NONE);} else {controller.dispatch(QuotaFilterAction::Apply);}
        })).into_any(),
    ]
}
