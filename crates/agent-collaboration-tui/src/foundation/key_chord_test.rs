use agent_collaboration_tui::foundation::KeyChord;
use iocraft::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
#[test]
fn raw_unit_separator_normalizes_to_the_printable_control_seven_chord() {
    let printable: KeyChord = "Ctrl+7".parse().unwrap();
    let raw = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('\u{1f}'));
    assert!(
        printable.matches(&raw),
        "raw US must match the printable binding"
    );
    let raw_binding: KeyChord = "\u{1f}".parse().unwrap();
    assert_eq!(raw_binding.to_string(), "Ctrl+7");
    assert!(!raw_binding.to_string().chars().any(char::is_control));
    assert_eq!(raw_binding, printable);
}

#[test]
fn key_parser_matches_aliases_but_never_repeat_or_release_effects() {
    for (text, code, modifiers, display) in [
        (
            "control+shift+enter",
            KeyCode::Enter,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            "Ctrl+Shift+Enter",
        ),
        ("option+f5", KeyCode::F(5), KeyModifiers::ALT, "Alt+F5"),
        ("cmd+k", KeyCode::Char('K'), KeyModifiers::SUPER, "Super+K"),
        (
            "backtab",
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
            "Shift+Tab",
        ),
    ] {
        let chord: KeyChord = text.parse().unwrap();
        assert_eq!(chord.to_string(), display);
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            let mut event = KeyEvent::new(kind, code);
            event.modifiers = modifiers;
            assert_eq!(chord.matches(&event), kind == KeyEventKind::Press);
        }
    }
    for invalid in ["", "ctrl", "a+b", "f25", "ctrl+unknown"] {
        assert!(invalid.parse::<KeyChord>().is_err(), "{invalid}");
    }
}
