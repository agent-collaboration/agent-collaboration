use super::demo_shell_state::{
    DemoOverlay, DemoShellState, DetailsFocus, DialogFocus, FAKE_SESSIONS, SessionKey, SessionSort,
    ShellFocus, overlay_rectangle,
};
use iocraft::prelude::*;

impl DemoShellState {
    pub(super) fn reconcile_selection(&mut self) {
        let sessions = self.visible_sessions();
        if !sessions
            .iter()
            .any(|session| Some(session.key) == self.selected_session)
        {
            self.selected_session = sessions.first().map(|session| session.key);
        }
    }
    fn move_session(&mut self, down: bool) {
        let sessions = self.visible_sessions();
        let current = sessions
            .iter()
            .position(|session| Some(session.key) == self.selected_session)
            .unwrap_or(0);
        let next = if down {
            current
                .saturating_add(1)
                .min(sessions.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.selected_session = sessions.get(next).map(|session| session.key);
    }
    fn show_details(&mut self) {
        if let Some(key) = self.selected_session {
            self.overlays.push(DemoOverlay::Details {
                session: key,
                focus: DetailsFocus::Open,
            });
            self.last_action = "Details opened".to_owned();
        }
    }
    pub(super) fn pretend_session_action(&mut self, key: SessionKey, fork: bool) {
        if let Some(session) = FAKE_SESSIONS.iter().find(|session| session.key == key) {
            self.last_action = format!(
                "would {} {}",
                if fork { "fork" } else { "open" },
                session.title
            );
            self.overlays.clear();
        }
    }
    pub(super) fn start_session(&mut self) {
        self.last_action = format!("Started on {}", self.selected_machine.label());
        self.overlays.clear();
        self.focus = ShellFocus::List;
    }
    fn open_dialog(&mut self) {
        self.overlays.push(DemoOverlay::NewSession {
            focus: DialogFocus::Client,
        });
        self.last_action = "New session dialog".to_owned();
    }
    fn focus_filter(&mut self) {
        self.focus = ShellFocus::Filter;
        self.last_action = "Filter focused".to_owned();
    }
    fn open_menu(&mut self) {
        self.overlays.push(DemoOverlay::Menu {
            selected: 0,
            return_focus: self.focus,
        });
        self.focus = ShellFocus::Menu;
        self.last_action = "Menu opened".to_owned();
    }
    fn close_menu(&mut self) {
        if let Some(DemoOverlay::Menu { return_focus, .. }) = self.overlays.pop() {
            self.focus = return_focus;
        }
    }
    fn run_menu_item(&mut self, selected: usize) {
        match selected {
            0 => self.open_dialog(),
            1 => {
                self.close_menu();
                self.focus_filter();
            }
            2 | 3 => {
                self.sort = if selected == 2 {
                    SessionSort::Updated
                } else {
                    SessionSort::Name
                };
                self.last_action = format!("Sort: {:?}", self.sort);
                self.close_menu();
            }
            4 => {
                self.overlays.push(DemoOverlay::Help);
                self.last_action = "Help opened".to_owned();
            }
            _ => {
                self.close_menu();
                self.last_action = "Menu closed".to_owned();
            }
        }
    }
    fn activate_focus(&mut self) {
        match self.focus {
            ShellFocus::Menu => self.open_menu(),
            ShellFocus::New => self.open_dialog(),
            ShellFocus::Filter => self.focus_filter(),
            ShellFocus::Scope => {
                self.this_folder = !self.this_folder;
                self.reconcile_selection();
                self.last_action = if self.this_folder {
                    "This folder"
                } else {
                    "All folders"
                }
                .to_owned();
            }
            ShellFocus::List => self.show_details(),
        }
    }
    pub fn handle_event(&mut self, event: TerminalEvent) {
        match event {
            TerminalEvent::Resize(width, height) => {
                self.width = width;
                self.height = height;
            }
            TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.quit_requested = true;
                    return;
                }
                if self.width < 76 || self.height < 24 {
                    return;
                }
                match self.overlays.last().cloned() {
                    None => self.background_key(key),
                    Some(DemoOverlay::Menu { selected, .. }) => self.menu_key(selected, key),
                    Some(DemoOverlay::Help) => {
                        if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::Enter) {
                            self.overlays.pop();
                            self.last_action = "Help closed".to_owned();
                        }
                    }
                    Some(overlay) => self.dialog_key(overlay, key),
                }
            }
            TerminalEvent::FullscreenMouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
            {
                if self.width < 76 || self.height < 24 {
                    return;
                }
                match self.overlays.last().cloned() {
                    None => self.background_click(mouse),
                    Some(overlay) => self.overlay_click(overlay, mouse),
                }
            }
            TerminalEvent::BracketedPaste(text) => {
                if self.overlays.is_empty() && self.focus == ShellFocus::Filter {
                    self.append_filter(&text);
                } else {
                    self.append_choice_query(&text);
                }
            }
            _ => {}
        }
    }
    fn append_filter(&mut self, text: &str) {
        self.filter_query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.reconcile_selection();
        self.last_action = format!("Filtered: {}", self.filter_query);
    }
    fn background_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Enter
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(self.focus, ShellFocus::List | ShellFocus::Filter)
        {
            if let Some(session) = self.selected_session {
                self.pretend_session_action(session, key.modifiers.contains(KeyModifiers::SHIFT));
            }
            return;
        }
        if key.code == KeyCode::Esc {
            if !self.filter_query.is_empty() {
                self.filter_query.clear();
                self.reconcile_selection();
                self.last_action = "Filter cleared".to_owned();
            } else if self.this_folder {
                self.this_folder = false;
                self.reconcile_selection();
                self.last_action = "Filters reset".to_owned();
            }
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = self.focus.advance(
                key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
            );
            return;
        }
        if matches!(key.code, KeyCode::Left | KeyCode::Right) {
            if self.focus == ShellFocus::Scope {
                self.this_folder = key.code == KeyCode::Left;
                self.reconcile_selection();
            } else {
                self.focus = self.focus.advance(key.code == KeyCode::Left);
            }
            return;
        }
        match key.code {
            KeyCode::F(10) => self.open_menu(),
            KeyCode::F(1) => {
                self.overlays.push(DemoOverlay::Help);
                self.last_action = "Help opened".to_owned();
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_dialog()
            }
            KeyCode::Char('/') if self.focus != ShellFocus::Filter => self.focus_filter(),
            KeyCode::Backspace if self.focus == ShellFocus::Filter => {
                self.filter_query.pop();
                self.reconcile_selection();
                self.last_action = format!("Filtered: {}", self.filter_query);
            }
            KeyCode::Enter if self.focus == ShellFocus::Filter => {
                self.show_details();
                self.focus = ShellFocus::List;
            }
            KeyCode::Char(character)
                if self.focus == ShellFocus::Filter
                    && !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
            {
                self.append_filter(&character.to_string())
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_focus(),
            KeyCode::Up | KeyCode::Down => {
                if self.focus == ShellFocus::List {
                    self.move_session(key.code == KeyCode::Down);
                } else {
                    self.focus = ShellFocus::List;
                }
            }
            _ => {}
        }
    }
    fn menu_key(&mut self, selected: usize, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::F(10) => {
                self.close_menu();
                self.last_action = "Menu closed".to_owned();
            }
            KeyCode::F(1) => {
                self.overlays.push(DemoOverlay::Help);
            }
            KeyCode::Up | KeyCode::Down => {
                let next = if key.code == KeyCode::Up {
                    selected.saturating_sub(1)
                } else {
                    selected.saturating_add(1).min(5)
                };
                if let Some(DemoOverlay::Menu { selected, .. }) = self.overlays.last_mut() {
                    *selected = next;
                }
            }
            KeyCode::Enter => self.run_menu_item(selected),
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_dialog()
            }
            KeyCode::Char('/') => self.run_menu_item(1),
            _ => {}
        }
    }
    fn background_click(&mut self, mouse: FullscreenMouseEvent) {
        if mouse.row == 0 {
            if (2..=11).contains(&mouse.column) {
                self.open_menu();
                return;
            }
            self.focus = match mouse.column {
                13..=21 => ShellFocus::New,
                23..=35 => ShellFocus::Filter,
                37..=57 => ShellFocus::Scope,
                _ => return,
            };
            self.activate_focus();
        } else if mouse.row >= 2
            && (mouse.row - 2) % 3 < 2
            && mouse.column >= 2
            && let Some(session) = self
                .visible_sessions()
                .get(usize::from((mouse.row - 2) / 3))
        {
            self.selected_session = Some(session.key);
            self.focus = ShellFocus::List;
            self.show_details();
        }
    }
    fn overlay_click(&mut self, overlay: DemoOverlay, mouse: FullscreenMouseEvent) {
        let rect = overlay_rectangle(&overlay);
        if !rect.contains(&mouse) {
            if matches!(
                overlay,
                DemoOverlay::Menu { .. } | DemoOverlay::ChoicePicker(_)
            ) {
                if matches!(overlay, DemoOverlay::Menu { .. }) {
                    self.close_menu();
                } else {
                    self.overlays.pop();
                }
                self.last_action = "Overlay dismissed".to_owned();
            }
            return;
        }
        let row = mouse.row - rect.top;
        let column = mouse.column - rect.left;
        match overlay {
            DemoOverlay::Menu { .. } if (2..8).contains(&row) => {
                self.run_menu_item(usize::from(row - 2))
            }
            DemoOverlay::NewSession { .. } => {
                if (4..7).contains(&row) && (3..49).contains(&column) {
                    self.open_choice(match row {
                        4 => DialogFocus::Client,
                        5 => DialogFocus::Machine,
                        _ => DialogFocus::Directory,
                    });
                } else if row == 12 {
                    if (3..25).contains(&column) {
                        self.overlays.pop();
                        self.last_action = "New session cancelled".to_owned();
                    } else if (28..50).contains(&column) {
                        self.start_session();
                    }
                }
            }
            DemoOverlay::ChoicePicker(picker) if row >= 3 => {
                if let Some(option) = picker.visible_options().get(usize::from(row - 3)) {
                    if let Some(DemoOverlay::ChoicePicker(current)) = self.overlays.last_mut() {
                        current.highlight =
                            super::demo_shell_choices::ChoiceHighlight::Item(option.key);
                    }
                    if option.availability
                        == super::demo_shell_choices::ChoiceAvailability::Available
                    {
                        self.pick_choice(option.key);
                    }
                }
            }
            DemoOverlay::Details { session, .. } if row == 16 => match column {
                3..=12 => {
                    self.overlays.pop();
                }
                16..=31 => self.pretend_session_action(session, false),
                35..=50 => self.pretend_session_action(session, true),
                _ => {}
            },
            _ => {}
        }
    }
}
