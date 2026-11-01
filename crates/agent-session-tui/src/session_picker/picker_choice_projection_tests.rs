use super::*;

#[test]
fn projection_preserves_the_typed_key_and_literal_presentation() {
    let choices = project_choices(vec![PickerChoice {
        key: PickerChoiceValue::Run(NewSessionClient::ClaudeNativeBackground),
        label: "Background".into(),
        secondary: Some("captured source".into()),
        availability: Availability::Available,
    }])
    .unwrap();
    assert_eq!(
        choices[0].option,
        SelectOption {
            key: OptionKey::new("run:ClaudeNativeBackground").unwrap(),
            label: "Background".into(),
            secondary: Some("captured source".into()),
            availability: Availability::Available
        }
    );
    assert_eq!(
        resolve_choice(
            &choices,
            &OptionKey::new("run:ClaudeNativeBackground").unwrap()
        )
        .unwrap(),
        PickerChoiceValue::Run(NewSessionClient::ClaudeNativeBackground)
    );
}

#[test]
fn projection_rejects_an_unknown_key_without_choosing_a_candidate() {
    let choices = project_choices(vec![PickerChoice::available(
        PickerChoiceValue::Client(NewSessionClient::CodexNative),
        "Codex",
    )])
    .unwrap();
    let error = resolve_choice(&choices, &OptionKey::new("client:unknown").unwrap()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Unknown picker choice key: client:unknown"
    );
}

#[test]
fn projection_rejects_a_known_unavailable_key_and_keeps_its_reason() {
    let choices = project_choices(vec![PickerChoice {
        key: PickerChoiceValue::Client(NewSessionClient::CursorManagedAcp),
        label: "Cursor · managed ACP".into(),
        secondary: None,
        availability: Availability::Unavailable {
            reason: "Captured source is unavailable".into(),
        },
    }])
    .unwrap();
    assert_eq!(
        choices[0].option.availability,
        Availability::Unavailable {
            reason: "Captured source is unavailable".into()
        }
    );
    let error = resolve_choice(
        &choices,
        &OptionKey::new("client:CursorManagedAcp").unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Picker choice is unavailable: Captured source is unavailable"
    );
}
