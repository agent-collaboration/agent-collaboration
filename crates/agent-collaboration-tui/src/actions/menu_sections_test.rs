use crate::{
    actions::{ActionError, ActionId, ActionRegistry, MenuSectionSpec, Placement, ScopeId},
    test_support::core_models::action,
};
fn section(name: &'static str, heading: Option<&str>) -> MenuSectionSpec {
    MenuSectionSpec {
        section: name,
        heading_action: heading.map(|name| ActionId::new(name).unwrap()),
    }
}
fn registry() -> ActionRegistry {
    let mut cycle = action("cycle", "Ctrl+S");
    cycle.placement = Placement::Hidden;
    ActionRegistry::new(ScopeId::new("section-validation").unwrap(), vec![cycle]).unwrap()
}
#[test]
fn menu_sections_reject_blank_duplicate_or_missing_heading_declarations() {
    for (sections, expected, description) in [
        (
            vec![section(" \t", None)],
            ActionError::BlankMenuSection,
            "blank section names are rejected",
        ),
        (
            vec![section("Scope", None), section("Scope", None)],
            ActionError::DuplicateMenuSection("Scope"),
            "duplicate section names are rejected",
        ),
        (
            vec![section("Scope", Some("foreign"))],
            ActionError::MissingMenuHeading(ActionId::new("foreign").unwrap()),
            "missing heading actions are rejected",
        ),
        (
            vec![
                section("Scope", Some("cycle")),
                section("Other", Some("cycle")),
            ],
            ActionError::DuplicateMenuHeading(ActionId::new("cycle").unwrap()),
            "duplicate heading actions are rejected",
        ),
    ] {
        let result = registry().with_menu_sections(sections);
        assert_eq!(result.as_ref().err(), Some(&expected), "{description}");
    }
    let registry = registry()
        .with_menu_sections(vec![
            section("Scope", Some("cycle")),
            section("Empty", None),
        ])
        .unwrap();
    assert_eq!(
        registry.menu_sections(),
        &[section("Scope", Some("cycle")), section("Empty", None)]
    );
    assert_eq!(
        registry.definitions().len(),
        1,
        "metadata adds no action or copied binding"
    );
}
