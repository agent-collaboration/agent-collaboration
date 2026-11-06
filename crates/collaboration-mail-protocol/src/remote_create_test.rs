use super::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const ID: &str = "019f0000-0000-7000-8000-000000000001";
fn session() -> Value {
    json!({"endpoint":{"serviceId":ID,"endpointId":"codex-local"},"sessionId":"requester"})
}
fn command() -> Value {
    json!({"endpoint":"codex-local","workingDirectory":"/tmp","access":"workspace-write",
        "model":"provider-model","effort":"high","mode":"agent","firstInput":"hello"})
}
fn assert_closed<T: DeserializeOwned + Serialize>(fixtures: Vec<Value>) {
    for fixture in fixtures {
        let decoded: T = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), fixture);
        let mut extra = fixture;
        extra["extra"] = json!(true);
        assert!(
            serde_json::from_value::<T>(extra.clone()).is_err(),
            "accepted extra fields: {extra}"
        );
    }
    assert!(serde_json::from_value::<T>(json!({"kind":"futureVariant"})).is_err());
}

#[test]
fn remote_create_contract_round_trips_every_closed_variant_and_refuses_unknown_fields() {
    assert_closed::<RemoteCreateRequest>(vec![
        json!({"requestId":ID,"requester":session(),"targetMachine":ID,"command":command()}),
    ]);
    assert_closed::<CreateSessionCommand>(vec![command()]);
    assert_closed::<RemoteCreateAdmission>(vec![
        json!({"kind":"accepted","requestId":ID}),
        json!({"kind":"replayed","requestId":ID,"status":{"kind":"admitted"}}),
        json!({"kind":"refused","reason":{"kind":"targetOffline"}}),
    ]);
    assert_closed::<RemoteCreateRefusal>(
        [
            "senderNotEnrolled",
            "targetNotEnrolled",
            "targetOffline",
            "conflict",
            "tooOld",
        ]
        .map(|kind| json!({"kind":kind}))
        .to_vec(),
    );
    assert_closed::<CreateSessionResult>(vec![
        json!({"kind":"created","session":session(),"settings":{"kind":"allApplied"},"firstInput":{"kind":"notRequested"}}),
        json!({"kind":"refused","reason":{"kind":"noRemoteApprover"}}),
        json!({"kind":"unknown"}),
    ]);
    assert_closed::<SettingsOutcome>(vec![
        json!({"kind":"allApplied"}),
        json!({"kind":"partial","notApplied":["model","effort","mode","access"]}),
    ]);
    assert_closed::<FirstInputOutcome>(vec![
        json!({"kind":"notRequested"}),
        json!({"kind":"notAttempted"}),
        json!({"kind":"accepted"}),
        json!({"kind":"unknown"}),
        json!({"kind":"failed","reason":{"kind":"refused","code":"pre_effect.refused-1","detail":"refused before dispatch"}}),
    ]);
    assert_closed::<RemoteCreateStatus>(vec![
        json!({"kind":"admitted"}),
        json!({"kind":"atTarget"}),
        json!({"kind":"completed","result":{"kind":"unknown"}}),
        json!({"kind":"expired"}),
    ]);
    assert_closed::<RemoteCreateOutcome>(vec![
        json!({"kind":"completed","value":{"kind":"unknown"}}),
        json!({"kind":"expired"}),
    ]);
    assert_closed::<CreateRefusal>(vec![
        json!({"kind":"noRemoteApprover"}),
        json!({"kind":"endpointUnavailable"}),
        json!({"kind":"invalidWorkingDirectory"}),
        json!({"kind":"invalidSettings"}),
        json!({"kind":"providerRefused","code":"refused","detail":"provider refused"}),
    ]);
    assert_closed::<PromptFailure>(vec![
        json!({"kind":"refused","code":"refused","detail":"prompt refused"}),
    ]);
    for setting in ["model", "effort", "mode", "access"] {
        let parsed: SettingName = serde_json::from_value(json!(setting)).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(setting));
    }
    assert!(serde_json::from_value::<SettingName>(json!("futureSetting")).is_err());
}

#[test]
fn bounded_code_and_text_validate_construction_and_untrusted_decoding_in_bytes() {
    for code in ["a".repeat(64), "az09_.-".to_owned()] {
        assert!(BoundedCode::new(code.clone()).is_ok());
        assert!(serde_json::from_value::<BoundedCode>(json!(code)).is_ok());
    }
    for code in [
        "".to_owned(),
        "a".repeat(65),
        "UPPER".to_owned(),
        "space code".to_owned(),
        "é".to_owned(),
        "line\n".to_owned(),
    ] {
        assert!(BoundedCode::new(code.clone()).is_err());
        assert!(serde_json::from_value::<BoundedCode>(json!(code)).is_err());
    }
    for text in ["x".repeat(1024), "é".repeat(512), "\u{1}".repeat(1024)] {
        assert!(BoundedText::new(text.clone()).is_ok());
        assert!(serde_json::from_value::<BoundedText>(json!(text)).is_ok());
    }
    for text in [String::new(), "x".repeat(1025), "é".repeat(513)] {
        assert!(BoundedText::new(text.clone()).is_err());
        assert!(serde_json::from_value::<BoundedText>(json!(text)).is_err());
    }
    let mut invalid = command();
    invalid["workingDirectory"] = json!("relative");
    assert!(serde_json::from_value::<CreateSessionCommand>(invalid).is_err());
    let mut invalid = command();
    invalid["access"] = json!("futureAccess");
    assert!(serde_json::from_value::<CreateSessionCommand>(invalid).is_err());
}

#[test]
fn new_mail_bodies_and_acceptance_preserve_the_closed_shapes() {
    assert_closed::<crate::MailSender>(vec![json!({"kind":"fleet"})]);
    assert!(
        !crate::MailSender::Fleet
            .matches_submitting_installation(&ID.to_owned().try_into().unwrap())
    );
    assert_closed::<CreateSessionBody>(vec![
        json!({"requestId":ID,"requester":session(),"command":command()}),
    ]);
    assert_closed::<RemoteCreateResultBody>(vec![
        json!({"requestId":ID,"outcome":{"kind":"expired"}}),
    ]);
    let acceptance = json!({"createSession":{"kind":"unknown"}});
    let parsed: crate::CarrierAcceptance = serde_json::from_value(acceptance.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), acceptance);
}

#[test]
fn maximum_escaped_result_and_conservative_two_text_budget_fit_64_kib() {
    let mut maximum_session = session();
    maximum_session["endpoint"]["endpointId"] = json!("a".repeat(64));
    maximum_session["sessionId"] = json!("\u{1}".repeat(4096));
    let failure = json!({"kind":"refused","code":"a".repeat(64),"detail":"\u{1}".repeat(1024)});
    let created: CreateSessionResult = serde_json::from_value(json!({"kind":"created",
        "session":maximum_session,"settings":{"kind":"partial","notApplied":["model","effort","mode","access"]},
        "firstInput":{"kind":"failed","reason":failure}})).unwrap();
    let acceptance = crate::CarrierAcceptance::CreateSession(Box::new(created));
    let bytes = serde_json::to_vec(&acceptance).unwrap().len();
    // The closed Created shape carries one failure detail. Reserve a second
    // maximally escaped detail as the PD's conservative two-text headroom.
    let second_text_bytes = serde_json::to_vec(&"\u{1}".repeat(1024)).unwrap().len();
    assert!(
        bytes + second_text_bytes < 64 * 1024,
        "{bytes} + {second_text_bytes}"
    );
    let refused: CreateSessionResult = serde_json::from_value(json!({"kind":"refused","reason":{"kind":"providerRefused","code":"a".repeat(64),"detail":"\u{1}".repeat(1024)}})).unwrap();
    assert!(serde_json::to_vec(&refused).unwrap().len() < 64 * 1024);
}

#[test]
fn partial_settings_cannot_exceed_the_four_names_in_the_acceptance_budget() {
    assert!(SettingsOutcome::partial(vec![SettingName::Model; 5]).is_err());
    assert!(
        SettingsOutcome::partial(vec![
            SettingName::Model,
            SettingName::Effort,
            SettingName::Mode,
            SettingName::Access
        ])
        .is_ok()
    );
    let oversized = SettingsOutcome::Partial {
        not_applied: vec![SettingName::Model; 5],
    };
    assert!(serde_json::to_vec(&oversized).is_err());
    assert!(
        serde_json::from_value::<SettingsOutcome>(json!({
            "kind":"partial", "notApplied":["model","effort","mode","access","model"]
        }))
        .is_err()
    );
}

#[test]
fn first_input_preserves_the_existing_native_one_mib_text_contract() {
    assert!(FirstInput::try_from("x".repeat(1024 * 1024)).is_ok());
    for value in [
        String::new(),
        "x".repeat(1024 * 1024 + 1),
        "bad\0input".to_owned(),
    ] {
        assert!(FirstInput::try_from(value).is_err());
    }
}
