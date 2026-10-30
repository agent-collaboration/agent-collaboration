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
fn dialog_arrows_walk_fields_and_plain_enter_picks_without_starting() {
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
    state.handle_event(press(KeyCode::Enter));
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
        17,
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
    let canvas = element!(View(width: 110, height: 30) { OverlaySurface(overlay: DemoOverlay::Menu { selected: 0, return_focus: ShellFocus::Menu }, machine: MachineChoice::Sunclaw, client: ClientChoice::Codex, directory: DirectoryChoice::Atlas, sort: SessionSort::Updated) }).render(Some(110));
    assert_eq!(canvas.cell(33, 3).and_then(CanvasCell::text), Some("N"));
}

#[test]
fn control_enter_on_dropdown_starts_without_committing_highlight() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(control(KeyCode::Char('n')));
    state.handle_event(press(KeyCode::Down));
    state.handle_event(press(KeyCode::Enter));
    state.handle_event(press(KeyCode::Down));
    state.handle_event(control(KeyCode::Enter));
    assert!(state.overlays.is_empty());
    assert_eq!(state.selected_machine, MachineChoice::Sunclaw);
    assert_eq!(state.last_action, "Started on Sunclaw");
}

#[test]
fn filter_enter_opens_match_and_escape_clears_then_resets_without_quitting() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(press(KeyCode::Char('/')));
    state.handle_event(TerminalEvent::BracketedPaste("stor".to_owned()));
    state.handle_event(press(KeyCode::Enter));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::Details {
            session: SessionKey::Storage,
            ..
        })
    ));
    state.handle_event(press(KeyCode::Esc));
    state.focus = ShellFocus::List;
    state.this_folder = true;
    state.handle_event(press(KeyCode::Esc));
    assert!(state.filter_query.is_empty());
    assert!(state.this_folder);
    state.handle_event(press(KeyCode::Esc));
    assert!(!state.this_folder);
    state.handle_event(press(KeyCode::Esc));
    assert!(!state.quit_requested);
}

#[test]
fn owner_main_frames_support_76_columns_without_a_shortcut_footer() {
    for (width, height) in [(120, 36), (76, 30)] {
        let frame = crate::render_demo_canvas(DemoShellState::new(width, height)).to_string();
        assert!(frame.contains("Sessions · 6/6"), "{frame}");
        assert!(!frame.contains("Ctrl+C quit"), "{frame}");
    }
}

#[test]
fn own_combo_query_preserves_a_still_matching_highlight() {
    let mut state = DemoShellState::new(100, 30);
    state.handle_event(control(KeyCode::Char('n')));
    state.handle_event(press(KeyCode::Down));
    state.handle_event(press(KeyCode::Enter));
    state.handle_event(press(KeyCode::Down));
    state.handle_event(TerminalEvent::BracketedPaste("sun".to_owned()));
    assert!(
        matches!(state.overlays.last(), Some(DemoOverlay::ChoicePicker(picker)) if picker.highlighted_key() == Some(DialogChoice::Machine(MachineChoice::Sunbook)))
    );
}

#[test]
fn main_enter_opens_details_and_modified_enter_only_simulates_open_or_fork() {
    let mut state = DemoShellState::new(76, 30);
    state.handle_event(press(KeyCode::Enter));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::Details {
            session: SessionKey::Atlas,
            ..
        })
    ));
    state.handle_event(press(KeyCode::Esc));
    state.handle_event(control(KeyCode::Enter));
    assert_eq!(state.last_action, "would open Atlas sketch");
    assert!(state.overlays.is_empty());
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    key.modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    state.handle_event(TerminalEvent::Key(key));
    assert_eq!(state.last_action, "would fork Atlas sketch");
    assert!(!state.quit_requested);
}

#[test]
fn combo_missing_external_key_requires_navigation_and_unavailable_cannot_be_picked() {
    use crate::demo_shell_choices::{ChoiceHighlight, ChoicePickerState};
    let mut picker = ChoicePickerState::new(DialogChoice::Machine(MachineChoice::Sunbook));
    picker.replace_options(
        picker
            .options
            .clone()
            .into_iter()
            .filter(|option| option.key != DialogChoice::Machine(MachineChoice::Sunbook))
            .collect(),
    );
    assert_eq!(picker.highlight, ChoiceHighlight::AwaitingNavigation);
    assert_eq!(picker.pickable_key(), None);
    picker.append_query("sun");
    assert_eq!(picker.pickable_key(), None);
    picker.move_highlight(true);
    assert_eq!(
        picker.pickable_key(),
        Some(DialogChoice::Machine(MachineChoice::Sunclaw))
    );
    let mut picker = ChoicePickerState::new(DialogChoice::Machine(MachineChoice::Sunbook));
    picker.append_query("build");
    assert_eq!(
        picker.highlighted_key(),
        Some(DialogChoice::Machine(MachineChoice::BuildBox))
    );
    assert_eq!(picker.pickable_key(), None);
    let mut state = DemoShellState::new(76, 30);
    state.overlays.push(DemoOverlay::NewSession {
        focus: DialogFocus::Machine,
    });
    state.overlays.push(DemoOverlay::ChoicePicker(picker));
    state.handle_event(press(KeyCode::Enter));
    assert_eq!(state.overlays.len(), 2);
    assert_eq!(state.selected_machine, MachineChoice::Sunclaw);
}

#[test]
fn grouped_frames_have_exact_section_gaps_and_dimmed_status_backgrounds() {
    use crate::demo_shell_theme::{BackgroundRole, StatusRole};
    use unicode_width::UnicodeWidthStr;
    for (width, height) in [(120, 36), (76, 30)] {
        let scenes = crate::demo_shell_captures::owner_scenes(width, height);
        let details_state = scenes
            .iter()
            .find(|(name, _)| *name == "details")
            .unwrap()
            .1
            .clone();
        let details = crate::render_demo_canvas(details_state.clone());
        assert!(details.to_string().contains("Details"));
        for row in [6, 10, 14, 19] {
            assert_eq!(details.get_text(12, row, 52, 1).trim(), "", "row {row}");
        }
        for row in [7, 8, 9, 11, 12, 13, 15, 16, 17, 18, 20] {
            assert!(
                !details.get_text(12, row, 52, 1).trim().is_empty(),
                "row {row}"
            );
        }
        for (row, role) in [
            (16, StatusRole::Available),
            (17, StatusRole::Warning),
            (18, StatusRole::Error),
        ] {
            assert_eq!(
                details.cell(27, row).unwrap().text_style().unwrap().color,
                Some(role.color(false))
            );
        }
        assert_eq!(
            details.cell(12, 7).unwrap().background_color,
            Some(BackgroundRole::Group.color(false))
        );
        let mut covered = details_state;
        covered.overlays.push(DemoOverlay::Help);
        let covered = crate::render_demo_canvas(covered);
        assert_eq!(
            covered.cell(12, 17).unwrap().background_color,
            Some(BackgroundRole::Group.color(true))
        );
        assert_eq!(
            covered.cell(27, 17).unwrap().text_style().unwrap().color,
            Some(StatusRole::Warning.color(true))
        );
        let new_state = scenes
            .iter()
            .find(|(name, _)| *name == "new-dropdown")
            .unwrap()
            .1
            .clone();
        let new_frame = crate::render_demo_canvas(new_state);
        assert!(
            new_frame
                .get_text(20, 18, 46, 1)
                .contains("Ctrl+Enter start · Esc close")
        );
        let mut uncovered_new = scenes
            .iter()
            .find(|(name, _)| *name == "new-dropdown")
            .unwrap()
            .1
            .clone();
        uncovered_new.overlays.pop();
        let uncovered_new = crate::render_demo_canvas(uncovered_new);
        for row in [7, 12, 16] {
            assert_eq!(uncovered_new.get_text(20, row, 46, 1).trim(), "");
        }
        for row in [8, 9, 10, 11, 13, 14, 15, 17, 18] {
            assert!(!uncovered_new.get_text(20, row, 46, 1).trim().is_empty());
        }
        assert_eq!(
            new_frame.cell(20, 8).unwrap().background_color,
            Some(BackgroundRole::Group.color(true))
        );
        assert_eq!(
            new_frame.cell(45, 17).unwrap().background_color,
            Some(BackgroundRole::Primary.color(true))
        );
        for (name, state) in scenes {
            let canvas = crate::render_demo_canvas(state);
            assert_eq!(canvas.height(), usize::from(height));
            assert_eq!(canvas.width(), usize::from(width), "{name}");
            assert!(canvas.cell(usize::from(width), 0).is_none());
            assert!(canvas.to_string().contains("Sessions · 6/6"), "{name}");
            assert!(
                canvas
                    .to_string()
                    .lines()
                    .all(|line| line.width() <= usize::from(width)),
                "{name}"
            );
        }
    }
}

#[test]
fn chrome_glyphs_share_one_column_in_both_width_tables() {
    use crate::demo_shell_theme::MENU_ICON;
    use unicode_width::UnicodeWidthStr;
    for glyph in [
        MENU_ICON, "▾", "❯", "●", "✓", "·", "│", "─", "┌", "┐", "└", "┘", "▏", "…", "↑", "↓",
    ] {
        // iocraft is locked to unicode-width0.1.14; its next-cell position
        // measures the old table through the exact renderer, without a new dep.
        let old_table = element!(View(width: 8, height: 1) { Text(content: format!("{glyph}#")) })
            .render(Some(8));
        let next_column = (0..8)
            .find(|column| old_table.cell(*column, 0).and_then(CanvasCell::text) == Some("#"));
        assert_eq!(next_column, Some(1), "renderer0.1.14 {glyph:?}");
        assert_eq!(glyph.width(), 1, "unicode-width0.2.2 {glyph:?}");
    }
}

#[test]
fn unavailable_pointer_target_can_highlight_without_picking() {
    use crate::demo_shell_choices::ChoicePickerState;
    let mut state = DemoShellState::new(76, 30);
    state.overlays.push(DemoOverlay::NewSession {
        focus: DialogFocus::Machine,
    });
    state
        .overlays
        .push(DemoOverlay::ChoicePicker(ChoicePickerState::new(
            DialogChoice::Machine(MachineChoice::Sunclaw),
        )));
    state.handle_event(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        23,
        15,
    )));
    assert_eq!(state.overlays.len(), 2);
    assert_eq!(state.selected_machine, MachineChoice::Sunclaw);
    assert!(
        matches!(state.overlays.last(), Some(DemoOverlay::ChoicePicker(picker)) if picker.highlighted_key() == Some(DialogChoice::Machine(MachineChoice::BuildBox)))
    );
}

#[test]
fn menu_escape_restores_the_target_for_each_opening_path() {
    for initial_focus in [ShellFocus::List, ShellFocus::Filter] {
        for by_click in [false, true] {
            let mut state = DemoShellState::new(76, 30);
            state.focus = initial_focus;
            state.handle_event(if by_click {
                TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::Down(MouseButton::Left),
                    4,
                    0,
                ))
            } else {
                press(KeyCode::F(10))
            });
            assert!(matches!(
                state.overlays.last(),
                Some(DemoOverlay::Menu { .. })
            ));
            state.handle_event(press(KeyCode::Esc));
            assert!(state.overlays.is_empty());
            assert_eq!(state.focus, initial_focus, "click={by_click}");
        }
    }
    let mut state = DemoShellState::new(76, 30);
    state.handle_event(press(KeyCode::Tab));
    assert_eq!(state.focus, ShellFocus::Menu);
    state.handle_event(press(KeyCode::Enter));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::Menu { .. })
    ));
    state.handle_event(press(KeyCode::Esc));
    assert!(state.overlays.is_empty());
    assert_eq!(state.focus, ShellFocus::Menu);
    state.handle_event(press(KeyCode::Char('/')));
    state.handle_event(press(KeyCode::Tab));
    assert_eq!(state.focus, ShellFocus::Scope);
    for _ in 0..2 {
        state.handle_event(press(KeyCode::Tab));
    }
    assert_eq!(state.focus, ShellFocus::Menu);
    state.handle_event(press(KeyCode::Enter));
    assert!(matches!(
        state.overlays.last(),
        Some(DemoOverlay::Menu { .. })
    ));
    state.handle_event(press(KeyCode::Esc));
    assert_eq!(state.focus, ShellFocus::Menu);
}

#[test]
fn menu_button_exposes_a_muted_f10_at_both_owner_sizes() {
    for (width, height) in [(120, 36), (76, 30)] {
        let canvas = crate::render_demo_canvas(DemoShellState::new(width, height));
        assert_eq!(canvas.get_text(2, 0, 10, 1), "≡ Menu F10");
        assert_eq!(
            canvas.cell(9, 0).unwrap().text_style().unwrap().color,
            Some(codex_router_tui::UiTextRole::Secondary.color())
        );
    }
}

#[test]
fn demo_reuses_the_shipped_surface_and_status_roles() {
    use crate::demo_shell_theme::{BackgroundRole, StatusRole};
    use codex_router_tui::{UiSurface, UiTextRole};
    for (demo, shipped) in [
        (BackgroundRole::Base, UiSurface::Base),
        (BackgroundRole::Surface, UiSurface::Base),
        (BackgroundRole::Group, UiSurface::Raised),
        (BackgroundRole::Focused, UiSurface::Selected),
        (BackgroundRole::Primary, UiSurface::Header),
    ] {
        assert_eq!(demo.color(false), shipped.color());
        assert_ne!(demo.color(true), shipped.color());
    }
    assert_eq!(
        StatusRole::Available.color(false),
        UiTextRole::Success.color()
    );
    assert_eq!(
        StatusRole::Warning.color(false),
        UiTextRole::Warning.color()
    );
    assert_eq!(StatusRole::Error.color(false), UiTextRole::Error.color());
}
