#[test]
fn default_help_names_original_page_and_bounds_keys_with_the_option_aliases() {
    use agent_collaboration_tui::dialog::help::{HelpContent, HelpInputConvention};
    assert_eq!(
        HelpContent::default().conventions,
        vec![
            HelpInputConvention {
                key: "PgUp/PgDn · Option+↑/↓".into(),
                description: "Page".into()
            },
            HelpInputConvention {
                key: "Home/End · Option+Shift+↑/↓".into(),
                description: "First/last".into()
            }
        ]
    );
}
