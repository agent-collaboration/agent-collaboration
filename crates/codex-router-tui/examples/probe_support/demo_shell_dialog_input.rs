use super::demo_shell_state::{DemoOverlay, DemoShellState, DialogChoice, DialogFocus};
use iocraft::prelude::*;

impl DemoShellState {
    pub(super) fn open_choice(&mut self, focus: DialogFocus) {
        let selected = match focus {
            DialogFocus::Client => DialogChoice::Client(self.selected_client),
            DialogFocus::Machine => DialogChoice::Machine(self.selected_machine),
            DialogFocus::Directory => DialogChoice::Directory(self.selected_directory),
            DialogFocus::Cancel | DialogFocus::Start => return,
        };
        self.overlays.push(DemoOverlay::ChoicePicker {
            query: String::new(),
            selected,
        });
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
        if let Some(DemoOverlay::ChoicePicker { query, selected }) = self.overlays.last_mut() {
            query.extend(text.chars().filter(|character| !character.is_control()));
            if let Some(first) = selected.matching(query).first() {
                *selected = *first;
            }
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
            DemoOverlay::ChoicePicker { query, selected } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "Choice list closed".to_owned();
                }
                KeyCode::Up | KeyCode::Down => {
                    let options = selected.matching(&query);
                    let current = options
                        .iter()
                        .position(|choice| *choice == selected)
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Up {
                        current.saturating_sub(1)
                    } else {
                        current
                            .saturating_add(1)
                            .min(options.len().saturating_sub(1))
                    };
                    if let (Some(choice), Some(DemoOverlay::ChoicePicker { selected, .. })) =
                        (options.get(next), self.overlays.last_mut())
                    {
                        *selected = *choice;
                    }
                }
                KeyCode::Enter => {
                    if selected.matching(&query).contains(&selected) {
                        self.pick_choice(selected);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(DemoOverlay::ChoicePicker { query, selected }) =
                        self.overlays.last_mut()
                    {
                        query.pop();
                        if let Some(first) = selected.matching(query).first() {
                            *selected = *first;
                        }
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
            DemoOverlay::Menu { .. } | DemoOverlay::Help => {}
        }
    }
}
