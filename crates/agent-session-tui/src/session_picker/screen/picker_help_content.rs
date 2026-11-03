//! Picker context and input conventions supplement generated action bindings.
use crate::session_picker::screen::picker_surface::PickerSurfaceContext;
use agent_collaboration_tui::dialog::help::HelpContent;
use agent_collaboration_tui::dialog::help::HelpContextLine;
use agent_collaboration_tui::dialog::help::HelpInputConvention;
pub(crate) fn help_content(context: &PickerSurfaceContext) -> HelpContent {
    let model = context.model.read();
    let mut content = HelpContent::default();
    content.context.push(HelpContextLine {
        label: "Screen".into(),
        value: context
            .desired_dialog()
            .map_or("Sessions".to_owned(), |kind| {
                match kind {
                    crate::session_picker::screen::picker_surface::PickerDialogKind::New => {
                        "New session"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Details => {
                        "Session details"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Filters => {
                        "Session filters"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Machine => {
                        "Choose machine"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Fork => {
                        "Fork session"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Source => {
                        "Checking source"
                    }
                    crate::session_picker::screen::picker_surface::PickerDialogKind::Client => {
                        "Client action"
                    }
                }
                .to_owned()
            }),
    });
    if let Some(dialog) = &model.details_dialog {
        for (label, value) in [
            ("Session", &dialog.record().title),
            ("Original title", &dialog.record().full_title),
            ("Session ID", &dialog.record().session_id),
        ] {
            content.context.push(HelpContextLine {
                label: label.into(),
                value: value.clone(),
            });
        }
        if let Some(name) = &dialog.record().explicit_name {
            content.context.push(HelpContextLine {
                label: "Name".into(),
                value: name.clone(),
            });
        }
    }
    if let Some(confirmation) = &model.fork_confirmation {
        content.context.push(HelpContextLine {
            label: "Session".into(),
            value: confirmation.title.clone(),
        });
        if let Some(id) = confirmation.session_id_label() {
            content.context.push(HelpContextLine {
                label: "Session ID".into(),
                value: id,
            });
        }
        content.context.push(HelpContextLine {
            label: "Original title".into(),
            value: confirmation.full_title.clone(),
        });
        if let Some(name) = &confirmation.explicit_name {
            content.context.push(HelpContextLine {
                label: "Name".into(),
                value: name.clone(),
            });
        }
    }
    content.conventions.extend(
        [
            ("Tab / Shift+Tab", "Move focus"),
            ("↑ / ↓", "Move field focus or list selection"),
            ("Typing / paste", "Edit the focused filter or text field"),
            (
                "Backspace / Delete",
                "Remove characters from the focused field",
            ),
            (
                "id: / b: / repo:",
                "Search by session ID, branch or repository",
            ),
            (
                "Esc",
                "Close dropdown or dialog; clear search, then reset filters; never quit",
            ),
        ]
        .into_iter()
        .map(|(key, description)| HelpInputConvention {
            key: key.into(),
            description: description.into(),
        }),
    );
    content
}
