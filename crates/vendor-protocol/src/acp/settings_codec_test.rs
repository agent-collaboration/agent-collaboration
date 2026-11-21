use super::{ObservedSettingValue, SettingCategory, observe_config_options};
use agent_client_protocol_schema::v1::{
    NewSessionResponse, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectGroup,
    SessionConfigSelectOption,
};

#[test]
fn omitted_session_options_are_distinct_from_reported_empty_options() {
    let omitted = NewSessionResponse::new("session");
    let empty = NewSessionResponse::new("session").config_options(Vec::new());

    assert_eq!(
        observe_config_options(omitted.config_options.as_deref()),
        None
    );
    assert_eq!(
        observe_config_options(empty.config_options.as_deref()),
        Some(Vec::new())
    );
}

#[test]
fn grouped_offers_keep_provider_order_and_do_not_add_the_selected_id() {
    let options = [SessionConfigOption::select(
        "Vendor/Custom",
        "Picker",
        "Current/Unlisted",
        vec![
            SessionConfigSelectGroup::new(
                "one",
                "First group",
                vec![
                    SessionConfigSelectOption::new("Choice/B", "B"),
                    SessionConfigSelectOption::new("Choice/A", "A"),
                ],
            ),
            SessionConfigSelectGroup::new(
                "two",
                "Second group",
                vec![SessionConfigSelectOption::new("Choice/C", "C")],
            ),
        ],
    )
    .category(SessionConfigOptionCategory::Other("_vendor_picker".into()))];

    let observed = observe_config_options(Some(&options)).expect("reported option set");

    assert_eq!(observed.len(), 1);
    let option = &observed[0];
    assert_eq!(option.id.0.as_ref(), "Vendor/Custom");
    assert_eq!(option.name, "Picker");
    assert_eq!(option.category, None);
    let ObservedSettingValue::Select { current, offered } = &option.value else {
        panic!("expected selected ID and choices");
    };
    assert_eq!(current.0.as_ref(), "Current/Unlisted");
    let offered = offered.as_deref().expect("reported choices");
    assert_eq!(
        offered
            .iter()
            .map(|choice| (choice.value.0.as_ref(), choice.name.as_str()))
            .collect::<Vec<_>>(),
        [("Choice/B", "B"), ("Choice/A", "A"), ("Choice/C", "C")]
    );
}

#[test]
fn cursor_thinking_boolean_and_effort_select_have_distinct_meanings() {
    for current in [false, true] {
        let options = [
            SessionConfigOption::boolean("thinking", "Thinking", current)
                .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::select(
                "effort",
                "Effort",
                "Current/Only",
                Vec::<SessionConfigSelectOption>::new(),
            )
            .category(SessionConfigOptionCategory::ModelConfig),
        ];

        let observed = observe_config_options(Some(&options)).expect("reported option set");

        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].id.0.as_ref(), "thinking");
        assert_eq!(observed[0].category, None);
        assert_eq!(observed[0].value, ObservedSettingValue::Boolean(current));
        assert_eq!(observed[1].id.0.as_ref(), "effort");
        assert_eq!(observed[1].category, Some(SettingCategory::Effort));
        let ObservedSettingValue::Select { current, offered } = &observed[1].value else {
            panic!("expected selected ID and choices");
        };
        assert_eq!(current.0.as_ref(), "Current/Only");
        assert_eq!(offered.as_deref(), Some([].as_slice()));
    }
}

#[test]
fn fast_model_config_keeps_its_category_and_boolean_value() {
    let options = [SessionConfigOption::boolean("fast", "Fast", false)
        .category(SessionConfigOptionCategory::ModelConfig)];

    let observed = observe_config_options(Some(&options)).expect("reported option set");

    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].id.0.as_ref(), "fast");
    assert_eq!(observed[0].category, Some(SettingCategory::Fast));
    assert_eq!(observed[0].value, ObservedSettingValue::Boolean(false));
}
