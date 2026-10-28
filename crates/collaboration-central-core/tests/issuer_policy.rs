use collaboration_central_core::AccessPolicy;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn a_private_network_issuer_has_an_independent_key_source() -> Result<(), Box<dyn std::error::Error>>
{
    let policy: AccessPolicy = serde_json::from_value(serde_json::json!({
        "issuer":"http://127.0.0.1:19843",
        "jwksUrl":"http://127.0.0.1:19843/.well-known/jwks.json",
        "sessionAudience":"router-session",
        "adminAudience":"router-admin",
        "ownerIdentity":"local-owner"
    }))?;
    policy.validate()?;
    assert_eq!(policy.issuer(), "http://127.0.0.1:19843");
    assert_eq!(
        policy.certs_url(),
        "http://127.0.0.1:19843/.well-known/jwks.json"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn cloudflare_configuration_keeps_its_exact_issuer_key_source_and_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let policy = AccessPolicy::cloudflare(
        "fixture.cloudflareaccess.com".into(),
        "session".into(),
        "admin".into(),
        "owner@example.invalid".into(),
    )?;
    assert_eq!(policy.issuer(), "https://fixture.cloudflareaccess.com");
    assert_eq!(
        policy.certs_url(),
        "https://fixture.cloudflareaccess.com/cdn-cgi/access/certs"
    );
    assert!(
        AccessPolicy::cloudflare(
            "example.invalid".into(),
            "session".into(),
            "admin".into(),
            "owner".into()
        )
        .is_err()
    );
    assert!(
        AccessPolicy::cloudflare(
            "https://fixture.cloudflareaccess.com".into(),
            "session".into(),
            "admin".into(),
            "owner".into()
        )
        .is_err()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn malformed_or_credential_bearing_issuer_and_key_source_are_refused()
-> Result<(), Box<dyn std::error::Error>> {
    for invalid in [
        "not a URL",
        "ftp://example.invalid",
        "http://user:secret@example.invalid",
        "https://example.invalid?secret=value",
        "https://example.invalid/#fragment",
    ] {
        for field in ["issuer", "jwksUrl"] {
            let mut value = serde_json::json!({"issuer":"https://central.example.invalid", "jwksUrl":"https://central.example.invalid/keys", "sessionAudience":"session", "adminAudience":"admin", "ownerIdentity":"local-owner"});
            value[field] = serde_json::json!(invalid);
            let policy: AccessPolicy = serde_json::from_value(value)?;
            assert!(policy.validate().is_err());
        }
    }
    Ok(())
}
