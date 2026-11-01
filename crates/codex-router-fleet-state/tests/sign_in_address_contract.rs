use codex_router_fleet_state::fleet_login::SignInAddress;
use fleet_identity_protocol::HttpsUrl;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::json;

// Wrong behavior: the saved address quietly retains the old URL parser's wider
// language or HTTPS-only restriction. Oracle: PD a2c79618's shared lexical rule.
// This relocation has no observed pre-fix red on the new crate path.
#[test]
fn sign_in_address_uses_the_shared_url_constructor_and_schema() {
    // Arrange
    let cases = [
        ("https://sign-in.example.invalid/api/auth".to_owned(), true),
        ("http://localhost:8787/".to_owned(), true),
        ("http://127.0.0.1:65535".to_owned(), true),
        ("http://[::1]:1/".to_owned(), true),
        ("http://sign-in.example.invalid".to_owned(), false),
        (
            "https://user:secret@sign-in.example.invalid".to_owned(),
            false,
        ),
        (
            "https://sign-in.example.invalid/#fragment".to_owned(),
            false,
        ),
        ("https://sign-in.example.invalid/?query=1".to_owned(), false),
        ("https://sign-in.example.invalid:0443".to_owned(), false),
        ("https://UPPER.example.invalid".to_owned(), false),
        ("https://bücher.example.invalid".to_owned(), false),
        (format!("https://a/{}", "a".repeat(2038)), true),
        (format!("https://a/{}", "a".repeat(2039)), false),
    ];

    for (input, expected) in cases {
        // Act
        let saved = serde_json::from_value::<SignInAddress>(json!(input));
        let shared = HttpsUrl::try_from(input.clone());

        // Assert
        assert_eq!(saved.is_ok(), expected, "saved address: {input}");
        assert_eq!(shared.is_ok(), expected, "shared address: {input}");
        if let Ok(saved) = saved {
            assert_eq!(saved.as_str(), input);
            assert_eq!(serde_json::to_value(saved).unwrap(), json!(input));
        }
    }
    assert_eq!(
        SignInAddress::json_schema(&mut SchemaGenerator::default()),
        HttpsUrl::json_schema(&mut SchemaGenerator::default()),
    );
}
