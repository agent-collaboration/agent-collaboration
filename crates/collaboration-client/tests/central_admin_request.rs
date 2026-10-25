//! Contract proof without sign-in, a Router, or network I/O.
#![allow(clippy::panic_in_result_fn)]
use collaboration_central_admin_protocol::*;
use collaboration_client::{CentralAdminClient, CentralAdminOperation};
use serde_json::{Value, json};
const PROJECT: &str = "019f0000-0000-7000-8000-000000005881";
#[test]
fn requests_bind_paths_methods_query_and_sensitive_owner_header()
-> Result<(), Box<dyn std::error::Error>> {
    let database: DatabaseId = serde_json::from_value(json!(PROJECT))?;
    let client = CentralAdminClient::new("https://central.example", "test-token-never-sent")?;
    for (operation, method, path) in [
        (
            CentralAdminOperation::Copies(CopyPageQuery {
                after: None,
                limit: None,
            }),
            "GET",
            "/admin/v1/copies".to_owned(),
        ),
        (
            CentralAdminOperation::Health(database),
            "GET",
            format!("/admin/v1/copies/{PROJECT}/health"),
        ),
        (
            CentralAdminOperation::Repair(database),
            "POST",
            format!("/admin/v1/copies/{PROJECT}/repair"),
        ),
        (
            CentralAdminOperation::Preview(database),
            "POST",
            format!("/admin/v1/projects/{PROJECT}/takeover/preview"),
        ),
    ] {
        let request = client.prepare(&operation)?;
        assert_eq!(request.method(), method);
        assert_eq!(request.url().path(), path);
        let access = request
            .headers()
            .get("cf-access-token")
            .ok_or("missing public Access credential")?;
        assert!(access.is_sensitive());
        assert_eq!(access.to_str()?, "test-token-never-sent");
        assert!(!request.headers().contains_key("Cf-Access-Jwt-Assertion"));
    }
    let request = client.prepare(&CentralAdminOperation::Migrate(serde_json::from_value(
        json!({"after":PROJECT,"limit":25}),
    )?))?;
    assert_eq!(request.method(), "POST");
    assert_eq!(
        request.url().query(),
        Some(format!("after={PROJECT}&limit=25").as_str())
    );
    Ok(())
}
#[test]
fn confirmation_serializes_the_full_native_preview_without_the_display_wrapper()
-> Result<(), Box<dyn std::error::Error>> {
    let body = json!({"transferId":PROJECT,"toInstallation":"550e8400-e29b-41d4-a716-446655440583","reason":"recover holder","preview":{"token":PROJECT,"fence":1,"acked":{"epoch":1,"sequence":7},"holder":"019f0000-0000-7000-8000-000000005583","lastSeen":1900000000000_i64}});
    let operation = CentralAdminOperation::Takeover {
        database: serde_json::from_value(json!(PROJECT))?,
        request: serde_json::from_value(body.clone())?,
    };
    let request = CentralAdminClient::new("https://central.example", "test-token-never-sent")?
        .prepare(&operation)?;
    assert_eq!(
        request.url().path(),
        format!("/admin/v1/projects/{PROJECT}/takeover")
    );
    let encoded: Value = serde_json::from_slice(
        request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .ok_or("missing confirmation body")?,
    )?;
    assert_eq!(encoded, body);
    Ok(())
}
#[test]
fn invalid_credential_destinations_are_refused_before_sign_in_or_http() {
    for url in [
        "file:///tmp/central",
        "https://user:password@central.example",
        "https://central.example?token=value",
        "https://central.example#fragment",
    ] {
        assert!(CentralAdminClient::validate_url(url).is_err());
    }
}
