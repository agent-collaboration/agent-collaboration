use super::*;
fn press(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control(code: KeyCode) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, code);
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}

#[test]
fn toolbar_filter_is_live_and_not_an_overlay() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(press(KeyCode::Char('/')));
    state.handle_event(press(KeyCode::Char('a')));
    state.handle_event(TerminalEvent::BracketedPaste("tlas".to_owned()));
    assert_eq!(state.filter_query, "atlas");
    assert_eq!(state.visible_sessions().len(), 1);
    assert!(state.overlays.is_empty());
    state.handle_event(press(KeyCode::Esc));
    assert_eq!(state.visible_sessions().len(), 6);
    assert_eq!(state.focus, ShellFocus::Filter);
    state.handle_event(press(KeyCode::Tab));
    assert_eq!(state.focus, ShellFocus::Scope);
}
#[test]
fn top_overlay_consumes_click_and_ready_burst_before_background() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(press(KeyCode::Char('/')));
    state.handle_event(press(KeyCode::F(10)));
    state.handle_event(press(KeyCode::Char('a')));
    state.handle_event(TerminalEvent::BracketedPaste("tlas".to_owned()));
    assert_eq!(state.selected_session, Some(SessionKey::Atlas));
    assert_eq!(state.filter_query, "");
    state.handle_event(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        80,
        8,
    )));
    assert!(state.overlays.is_empty());
    assert_eq!(state.last_action, "Overlay dismissed");
}
#[test]
fn toolbar_tab_arrows_and_activation_reach_controls_and_list() {
    let mut state = DemoShellState::new(100, 30);
    for expected in [
        ShellFocus::Menu,
        ShellFocus::New,
        ShellFocus::Filter,
        ShellFocus::Scope,
        ShellFocus::List,
    ] {
        state.handle_event(press(KeyCode::Tab));
        assert_eq!(state.focus, expected);
    }
    state.handle_event(press(KeyCode::BackTab));
    assert_eq!(state.focus, ShellFocus::Scope);
    state.handle_event(press(KeyCode::Left));
    assert!(state.this_folder);
    state.handle_event(press(KeyCode::Right));
    assert!(!state.this_folder);
    state.handle_event(press(KeyCode::BackTab));
    assert_eq!(state.focus, ShellFocus::Filter);
    state.handle_event(press(KeyCode::Left));
    assert_eq!(state.focus, ShellFocus::New);
    state.handle_event(press(KeyCode::Char(' ')));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::NewSession { .. })
    ));
}
#[test]
fn dialog_arrows_walk_all_fields_and_choice_enter_never_starts_under_combo() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(control(KeyCode::Char('n')));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::NewSession {
            focus: DialogFocus::Client
        })
    ));
    for expected in [
        DialogFocus::Machine,
        DialogFocus::Directory,
        DialogFocus::Cancel,
        DialogFocus::Start,
    ] {
        state.handle_event(press(KeyCode::Down));
        assert!(
            matches!(state.overlays.last(), Some(DemoOverlay::NewSession { focus }) if *focus == expected)
        );
    }
    state.handle_event(press(KeyCode::Up));
    state.handle_event(press(KeyCode::Up));
    state.handle_event(press(KeyCode::Up));
    state.handle_event(press(KeyCode::Enter));
    state.handle_event(control(KeyCode::Enter));
    assert_eq!(state.overlays.len(), 1);
    assert!(!state.last_action.starts_with("Started"));
    state.handle_event(press(KeyCode::Enter));
    state.handle_event(press(KeyCode::Esc));
    assert_eq!(state.overlays.len(), 1);
    state.handle_event(control(KeyCode::Enter));
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
    assert_eq!(state.overlays.len(), 1);
    state.handle_event(TerminalEvent::Key(KeyEvent::new(
        KeyEventKind::Repeat,
        KeyCode::Enter,
    )));
    assert_eq!(state.overlays.len(), 1);
}
#[test]
fn menu_shortcut_uses_the_right_edge_of_the_panel() {
    use crate::demo_shell_surfaces::OverlaySurface;
    let canvas = element!(View(width: 110, height: 30) { OverlaySurface(overlay: DemoOverlay::Menu { selected: 0 }, machine: MachineChoice::Sunclaw, client: ClientChoice::Codex, directory: DirectoryChoice::Atlas, sort: SessionSort::Updated) }).render(Some(110));
    assert_eq!(canvas.cell(33, 3).and_then(CanvasCell::text), Some("N"));
}
