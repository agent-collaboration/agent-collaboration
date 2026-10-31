//! The generated Help and every scope share the accepted terminal encodings.
pub(crate) const fn quota_help_keys() -> &'static [&'static str] {
    &["F1", "Ctrl+7", "Ctrl+_", "Ctrl+/"]
}

#[cfg(test)]
pub(crate) fn quota_help_key(code: iocraft::KeyCode, modifiers: iocraft::KeyModifiers) -> bool {
    let mut event = iocraft::KeyEvent::new(iocraft::KeyEventKind::Press, code);
    event.modifiers = modifiers;
    quota_help_keys().iter().any(|key| {
        key.parse::<agent_collaboration_tui::actions::KeyChord>()
            .is_ok_and(|key| key.matches(&event))
    })
}
