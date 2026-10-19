//! Content-sized, pointer-accessible provider controls in the existing picker frame.
use super::{
    interactive_row::InteractiveSessionChoiceRow, picker_model::SessionsPickerModel,
    provider_filter_model::ProviderFilterFocus,
};
use codex_router_tui::{DialogAction, UiHeading, UiHeadingLevel, UiText, UiTextRole};
use iocraft::prelude::*;

pub(super) fn render_provider_controls(
    model: &SessionsPickerModel,
    mut model_state: State<SessionsPickerModel>,
    width: usize,
) -> AnyElement<'static> {
    let provider = model.provider_filter.label();
    let label = super::picker_component::fit_line(
        &if width >= 32 {
            format!("Filters · {provider}")
        } else {
            provider.to_owned()
        },
        width.saturating_sub(2),
    );
    element! {
        InteractiveSessionChoiceRow(
            activation_handler: move |_| model_state.write().open_provider_filters(),
            activates_on_click: true,
        ) {
            View(width: 100pct, column_gap: 1) {
                Text(content: label, color: Color::Grey, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                Text(content: "▾", color: Color::DarkGrey, wrap: TextWrap::NoWrap)
            }
        }
    }.into_any()
}

pub(super) fn render_provider_chooser(
    model: &SessionsPickerModel,
    model_state: State<SessionsPickerModel>,
) -> Vec<AnyElement<'static>> {
    use super::provider_filter_model::SessionFilterValue;
    let Some(draft) = model.provider_filter_draft.as_ref() else {
        return Vec::new();
    };
    let mut controls = vec![
        element!(UiHeading(content:"Session client provider".to_owned(),level:UiHeadingLevel::Section)).into_any(),
        element!(UiText(content:format!("Draft: {}",draft.selection.label()),role:UiTextRole::Secondary)).into_any(),
        element!(UiText(content:format!("Current: {} · {}/{} sessions",model.provider_filter.label(),model.visible_record_len(),model.request.records.len()),role:UiTextRole::Secondary)).into_any(),
    ];
    for (field, label) in [
        (ProviderFilterFocus::Provider, "Client"),
        (ProviderFilterFocus::Folder, "Folder"),
        (ProviderFilterFocus::ModelProvider, "Model provider"),
        (ProviderFilterFocus::Source, "Source"),
        (ProviderFilterFocus::Runtime, "Runtime"),
        (ProviderFilterFocus::Sort, "Sort"),
    ] {
        let mut field_draft = draft.clone();
        field_draft.focus = field;
        let Some(value) = field_draft.focused_value() else {
            continue;
        };
        let options = super::provider_filter_model::filter_choices(model, field);
        let value_label = options
            .iter()
            .find(|option| option.key == value)
            .map(|option| option.label.clone())
            .unwrap_or_else(|| format!("{value:?}"));
        let expanded = draft.focus == field && model.provider_choice.is_open();
        let body = if expanded {
            element! {
                codex_router_tui::SearchSelect<SessionFilterValue>(state:model.provider_choice.clone(),options,
                    on_select:move |value| {
                        let mut model_state=model_state;
                        let mut model=model_state.write();
                        if let Some(draft)=model.provider_filter_draft.as_mut() {draft.select_value(value);}
                        model.provider_choice=codex_router_tui::SearchSelectState::Closed;
                    })
            }.into_any()
        } else {
            let mut model_state = model_state;
            element! {
                InteractiveSessionChoiceRow(activates_on_click:true,activation_handler:move |_| {
                    let mut model=model_state.write();
                    if let Some(draft)=model.provider_filter_draft.as_mut() {draft.focus=field;}
                    model.provider_choice.open(value.clone());
                }) {
                    Text(content:format!("{value_label} ▾"),color:if draft.focus==field {Color::Yellow}else {Color::White})
                }
            }.into_any()
        };
        controls.push(element! {
            codex_router_tui::FormField(label:label.to_owned(),label_width:14u16,has_focus:draft.focus==field&&!expanded) {#(body)}
        }.into_any());
    }
    controls
}

pub(super) fn render_provider_actions(
    model: &SessionsPickerModel,
    mut model_state: State<SessionsPickerModel>,
    apply_filters: Handler<()>,
) -> Vec<AnyElement<'static>> {
    let Some(draft) = model.provider_filter_draft.as_ref() else {
        return Vec::new();
    };
    vec![
        element!(DialogAction(keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,label:"Cancel",has_focus:draft.focus==ProviderFilterFocus::Cancel,emphasis:codex_router_tui::DialogActionEmphasis::Secondary,handler:move |_|model_state.write().provider_filter_draft=None)).into_any(),
        element!(DialogAction(keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,label:"Apply",has_focus:draft.focus==ProviderFilterFocus::Apply,handler:apply_filters)).into_any(),
    ]
}
