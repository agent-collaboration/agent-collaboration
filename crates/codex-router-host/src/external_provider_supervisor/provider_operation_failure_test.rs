use super::*;
use acp_client_runtime::ProviderSettingKind;
use collaboration_protocol::ProviderSettingName;

#[test]
fn invalid_setting_failure_names_each_setting_and_advertised_values() {
    for (setting, name, expected_setting) in [
        (ProviderSettingKind::Mode, "mode", ProviderSettingName::Mode),
        (
            ProviderSettingKind::Model,
            "model",
            ProviderSettingName::Model,
        ),
        (
            ProviderSettingKind::Effort,
            "effort",
            ProviderSettingName::Effort,
        ),
    ] {
        let failure = invalid_setting_failure(
            OperationId::generate(),
            None,
            setting,
            "requested".to_owned(),
            vec!["first".to_owned(), "second".to_owned()],
            acp_client_runtime::InvalidSettingSessionDisposition::Closed,
        );

        assert_eq!(
            String::from(failure.message.clone()),
            format!(
                "invalid provider setting {name}=\"requested\"; advertised: \"first\", \"second\"; new Session was closed"
            )
        );
        let invalid_setting = failure.invalid_setting.expect("invalid-setting detail");
        assert_eq!(invalid_setting.setting, expected_setting);
        assert_eq!(invalid_setting.value, "requested");
        assert_eq!(
            invalid_setting.advertised,
            vec!["first".to_owned(), "second".to_owned()]
        );
    }
}

#[test]
fn invalid_setting_failure_bounds_large_advertised_values_and_echo() {
    let value = format!("requested\n{}", "v".repeat(500));
    let advertised = (0..500)
        .map(|choice_index| format!("choice-{choice_index}-{}", "🧪".repeat(120)))
        .collect::<Vec<_>>();
    let failure = invalid_setting_failure(
        OperationId::generate(),
        None,
        ProviderSettingKind::Model,
        value.clone(),
        advertised.clone(),
        acp_client_runtime::InvalidSettingSessionDisposition::Closed,
    );

    let message = String::from(failure.message.clone());
    let bounded_value = value
        .chars()
        .take(MAX_SETTING_VALUE_DISPLAY_CHARS)
        .collect::<String>();
    let expected_displayed_value = format!("{bounded_value:?}…");
    assert!(message.len() <= 4096);
    assert!(message.starts_with(&format!(
        "invalid provider setting model={expected_displayed_value}; advertised: "
    )));
    assert!(message.ends_with("; new Session was closed"));
    assert!(!message.contains(&"v".repeat(121)));

    let displayed_summary = message
        .split_once("; advertised: ")
        .and_then(|(_, after_advertised)| after_advertised.split_once("; new Session was closed"))
        .map(|(summary, _)| summary)
        .expect("failure names the advertised choices and Session disposition");
    assert!(displayed_summary.len() <= MAX_ADVERTISED_SUMMARY_BYTES);
    assert!(displayed_summary.matches("\"choice-").count() <= MAX_ADVERTISED_CHOICES);
    let first_advertised_choice = advertised
        .first()
        .expect("the fixture advertises a first choice");
    let bounded_first_choice = first_advertised_choice
        .chars()
        .take(MAX_SETTING_VALUE_DISPLAY_CHARS)
        .collect::<String>();
    let expected_displayed_choice = format!("{bounded_first_choice:?}…");
    assert!(displayed_summary.starts_with(&expected_displayed_choice));

    let omitted_count = message
        .split("… (+")
        .nth(1)
        .and_then(|after_marker| {
            after_marker
                .split_once(" more; see invalidSetting.advertised)")
                .map(|(count, _)| count)
        })
        .and_then(|count| count.parse::<usize>().ok())
        .expect("truncation marker reports an omitted-choice count");
    assert!(omitted_count > 0);

    let invalid_setting = failure.invalid_setting.expect("invalid-setting detail");
    assert_eq!(invalid_setting.setting, ProviderSettingName::Model);
    assert_eq!(invalid_setting.value, value);
    assert_eq!(invalid_setting.advertised, advertised);
    assert_eq!(
        invalid_setting.session_disposition,
        InvalidSettingSessionDisposition::Closed
    );

    let short_advertised = (0..500)
        .map(|choice_index| format!("choice-{choice_index}"))
        .collect::<Vec<_>>();
    let count_limited_summary = format_bounded_advertised_values(&short_advertised);
    assert!(count_limited_summary.len() <= MAX_ADVERTISED_SUMMARY_BYTES);
    assert_eq!(
        count_limited_summary.matches("\"choice-").count(),
        MAX_ADVERTISED_CHOICES
    );
    assert!(count_limited_summary.contains("… (+468 more; see invalidSetting.advertised)"));
}

#[test]
fn advertised_values_debug_escape_quotes_and_control_characters() {
    let advertised = vec!["line\n\"quoted\"".to_owned()];

    assert_eq!(
        format_bounded_advertised_values(&advertised),
        r#""line\n\"quoted\"""#
    );
}
