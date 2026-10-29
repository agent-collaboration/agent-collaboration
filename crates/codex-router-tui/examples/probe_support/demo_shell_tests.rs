use super::*;
fn press(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[test]
fn top_overlay_consumes_click_and_ready_burst_before_background() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(press(KeyCode::F(10)));
    state.handle_event(press(KeyCode::Char('/')));
    state.handle_event(press(KeyCode::Char('a')));
    state.handle_event(TerminalEvent::BracketedPaste("tlas".to_owned()));
    assert_eq!(state.selected_session, Some(SessionKey::Atlas));
    assert_eq!(state.filter_query, "");
    assert!(
        matches!(state.overlays.last(), Some(DemoOverlay::Filter { query }) if query == "atlas")
    );
    state.handle_event(press(KeyCode::Enter));
    assert_eq!(state.visible_sessions().len(), 1);
    assert_eq!(state.overlays.len(), 1);
    let before = state.selected_session;
    state.handle_event(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        80,
        8,
    )));
    assert!(state.overlays.is_empty());
    assert_eq!(state.selected_session, before);
    assert_eq!(state.last_action, "Overlay dismissed");
}
#[test]
fn machine_escape_closes_only_top_and_control_enter_never_starts_under_combo() {
    let mut state = DemoShellState::new(100, 30);
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('n'));
    key.modifiers = KeyModifiers::CONTROL;
    state.handle_event(TerminalEvent::Key(key));
    state.handle_event(press(KeyCode::Enter));
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    key.modifiers = KeyModifiers::CONTROL;
    state.handle_event(TerminalEvent::Key(key.clone()));
    assert_eq!(state.overlays.len(), 1);
    assert!(!state.last_action.starts_with("Started"));
    state.handle_event(press(KeyCode::Enter));
    state.handle_event(press(KeyCode::Esc));
    assert_eq!(state.overlays.len(), 1);
    state.handle_event(TerminalEvent::Key(key));
    assert!(state.overlays.is_empty());
    assert_eq!(state.last_action, "Started on Sunclaw");
}

#[test]
fn button_gaps_and_repeat_commands_do_not_activate_actions() {
    let mut state = DemoShellState::new(100, 30);
    state.overlays.push(DemoOverlay::NewSession {
        focus: DialogFocus::Start,
    });
    state.handle_event(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        44,
        14,
    )));
    assert_eq!(state.overlays.len(), 1, "button gap must not Start");
    state.handle_event(TerminalEvent::Key(KeyEvent::new(
        KeyEventKind::Repeat,
        KeyCode::Enter,
    )));
    assert_eq!(state.overlays.len(), 1, "Repeat must not Start");
}

#[test]
fn menu_shortcut_uses_the_right_edge_of_the_panel() {
    use crate::demo_shell_surfaces::OverlaySurface;
    let canvas = element!(View(width: 110, height: 30) {
        OverlaySurface(overlay: DemoOverlay::Menu { selected: 0 }, machine: MachineChoice::Sunclaw, sort: SessionSort::Updated)
    }).render(Some(110));
    assert_eq!(canvas.cell(33, 3).and_then(CanvasCell::text), Some("N"));
}
