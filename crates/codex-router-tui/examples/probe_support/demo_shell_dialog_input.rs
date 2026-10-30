use super::demo_shell_choices::ChoicePickerState;
use super::demo_shell_state::{
    DemoOverlay, DemoShellState, DetailsFocus, DialogChoice, DialogFocus,
};
use iocraft::prelude::*;

impl DemoShellState {
    pub(super) fn open_choice(&mut self, focus: DialogFocus) {
        let selected = match focus {
            DialogFocus::Client => DialogChoice::Client(self.selected_client),
            DialogFocus::Machine => DialogChoice::Machine(self.selected_machine),
            DialogFocus::Directory => DialogChoice::Directory(self.selected_directory),
            DialogFocus::Cancel | DialogFocus::Start => return,
        };
        self.overlays
            .push(DemoOverlay::ChoicePicker(ChoicePickerState::new(selected)));
    }
    pub(super) fn pick_choice(&mut self, choice: DialogChoice) {
        match choice {
            DialogChoice::Client(client) => self.selected_client = client,
            DialogChoice::Machine(machine) => self.selected_machine = machine,
            DialogChoice::Directory(directory) => self.selected_directory = directory,
        }
        self.overlays.pop();
        self.last_action = format!("{}: {}", choice.field_label(), choice.label());
    }
    pub(super) fn append_choice_query(&mut self, text: &str) {
        if let Some(DemoOverlay::ChoicePicker(picker)) = self.overlays.last_mut() {
            picker.append_query(text);
        }
    }
    pub(super) fn dialog_key(&mut self, overlay: DemoOverlay, key: KeyEvent) {
        if key.code == KeyCode::F(1) {
            self.overlays.push(DemoOverlay::Help);
            return;
        }
        match overlay {
            DemoOverlay::NewSession { focus } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "New session cancelled".to_owned();
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.start_session()
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    if let Some(DemoOverlay::NewSession { focus: current }) =
                        self.overlays.last_mut()
                    {
                        *current = focus.advance(
                            matches!(key.code, KeyCode::BackTab | KeyCode::Up)
                                || key.modifiers.contains(KeyModifiers::SHIFT),
                        );
                    }
                }
                KeyCode::Enter => match focus {
                    DialogFocus::Client | DialogFocus::Machine | DialogFocus::Directory => {
                        self.open_choice(focus)
                    }
                    DialogFocus::Cancel => {
                        self.overlays.pop();
                        self.last_action = "New session cancelled".to_owned();
                    }
                    DialogFocus::Start => self.start_session(),
                },
                _ => {}
            },
            DemoOverlay::ChoicePicker(picker) => match key.code {
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.overlays.pop();
                    self.start_session();
                }
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "Choice list closed".to_owned();
                }
                KeyCode::Up | KeyCode::Down => {
                    if let Some(DemoOverlay::ChoicePicker(picker)) = self.overlays.last_mut() {
                        picker.move_highlight(key.code == KeyCode::Down);
                    }
                }
                KeyCode::Enter => {
                    if let Some(choice) = picker.pickable_key() {
                        self.pick_choice(choice);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(DemoOverlay::ChoicePicker(picker)) = self.overlays.last_mut() {
                        picker.backspace_query();
                    }
                }
                KeyCode::Char(character)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    self.append_choice_query(&character.to_string())
                }
                _ => {}
            },
            DemoOverlay::Details { session, focus } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "Details closed".to_owned();
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => self
                    .pretend_session_action(session, key.modifiers.contains(KeyModifiers::SHIFT)),
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                    if let Some(DemoOverlay::Details { focus: current, .. }) =
                        self.overlays.last_mut()
                    {
                        *current = focus.advance(
                            matches!(key.code, KeyCode::BackTab | KeyCode::Left)
                                || key.modifiers.contains(KeyModifiers::SHIFT),
                        );
                    }
                }
                KeyCode::Enter => match focus {
                    DetailsFocus::Close => {
                        self.overlays.pop();
                    }
                    DetailsFocus::Open => self.pretend_session_action(session, false),
                    DetailsFocus::Fork => self.pretend_session_action(session, true),
                },
                _ => {}
            },
            DemoOverlay::Menu { .. } | DemoOverlay::Help => {}
        }
    }
}
