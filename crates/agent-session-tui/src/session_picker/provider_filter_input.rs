//! The chooser owns keys while open, before browse or machine shortcuts.
use super::{picker_model::SessionsPickerModel, provider_filter_model::ProviderFilterFocus};
use iocraft::prelude::{KeyCode, KeyModifiers};

pub(super) fn handle_provider_filter_key(
    model: &mut SessionsPickerModel,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if model.provider_filter_draft.is_some() && model.provider_choice.is_open() {
        let field = model
            .provider_filter_draft
            .as_ref()
            .map_or(ProviderFilterFocus::Provider, |draft| draft.focus);
        let choices = super::provider_filter_model::filter_choices(model, field);
        if let agent_collaboration_tui::ChoiceTransition::Selected(selection) =
            model.provider_choice.handle_key(code, &choices)
            && let Some(draft) = model.provider_filter_draft.as_mut()
        {
            draft.select_value(selection);
        }
        return true;
    }
    if let Some(draft) = model.provider_filter_draft.as_mut() {
        match code {
            KeyCode::Up => draft.move_focus(true),
            KeyCode::Down => draft.move_focus(false),
            KeyCode::Tab => draft.move_focus(modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::BackTab => draft.move_focus(true),
            KeyCode::Esc => model.provider_filter_draft = None,
            KeyCode::Enter
                if !modifiers.intersects(
                    KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER,
                ) =>
            {
                if let Some(value) = draft.focused_value() {
                    model.provider_choice.open(value);
                } else if draft.focus == ProviderFilterFocus::Cancel {
                    model.provider_filter_draft = None;
                } else {
                    model.apply_provider_filters();
                }
            }
            _ => {}
        }
        return true;
    }
    if code == KeyCode::F(3)
        || code == KeyCode::Char('f') && modifiers.contains(KeyModifiers::CONTROL)
    {
        model.open_provider_filters();
        return true;
    }
    false
}
