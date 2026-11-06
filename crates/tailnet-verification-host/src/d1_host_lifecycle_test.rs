use super::*;

#[test]
fn no_input_auth_check_rejects_auth_required_or_an_existing_account() {
    require_no_client_auth(json!({"account":null,"requiresOpenaiAuth":false}))
        .expect("no client auth");
    assert!(require_no_client_auth(json!({"account":null,"requiresOpenaiAuth":true})).is_err());
    assert!(
        require_no_client_auth(json!({"account":{"type":"apiKey"},"requiresOpenaiAuth":false}))
            .is_err()
    );
    assert!(require_no_client_auth(json!({"account":null})).is_err());
    assert!(require_no_client_auth(json!({"requiresOpenaiAuth":false})).is_err());
}

#[test]
fn native_reply_is_correlated_and_errors_never_count_as_readiness() {
    let expected = "019f0000-0000-7000-8000-000000008101";
    assert_eq!(
        parse_answer(
            br#"{"id":"019f0000-0000-7000-8000-000000008101","result":{"userAgent":"fixture"}}"#,
            expected,
        )
        .expect("matching reply"),
        Some(json!({"userAgent":"fixture"}))
    );
    assert!(
        parse_answer(br#"{"method":"notice","params":{}}"#, expected)
            .expect("notification")
            .is_none()
    );
    for wrong in [
        br#"{"id":"019f0000-0000-7000-8000-000000008102","result":{}}"#.as_slice(),
        br#"{"id":"019f0000-0000-7000-8000-000000008101","error":{"code":-1}}"#.as_slice(),
        br#"{"id":"019f0000-0000-7000-8000-000000008101","method":"turn/start","params":{}}"#
            .as_slice(),
        br#"{"id":"019f0000-0000-7000-8000-000000008101"}"#.as_slice(),
    ] {
        assert!(parse_answer(wrong, expected).is_err());
    }
}

#[test]
fn readiness_configuration_has_no_inherited_login_or_credential_carrier() {
    let config = native_config(28787);
    assert!(config.contains("requires_openai_auth = false"));
    assert!(config.contains("cli_auth_credentials_store = \"ephemeral\""));
    assert!(config.contains("http://127.0.0.1:28787/v1"));
    assert!(!config.contains("env_key"));
    assert!(!config.contains("token"));
}
