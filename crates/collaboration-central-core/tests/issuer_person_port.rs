//! Consumer regressions for the person-token boundary F3a implements.
#![cfg(test)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::organization_authority::ParsedPersonToken;
use serde_json::{Value, json};

fn structural_person_token(header: &Value) -> String {
    let producer: Value =
        serde_json::from_str(include_str!("fixtures/person-identity/K6.fixtures.json")).unwrap();
    let claims = &producer["examples"]["providerSession"];
    assert!(claims.get("sub").is_some());
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap()),
        URL_SAFE_NO_PAD.encode([1_u8, 2, 3]),
    )
}

// RFC7515 section4.1.11 requires unknown critical extensions to be refused.
// This is a parser observation only; the signature bytes are not a crypto proof.
#[test]
fn the_person_reader_refuses_unsupported_critical_headers() {
    let ordinary = json!({"typ":"at+jwt","alg":"EdDSA","kid":"fixture-key"});
    assert!(ParsedPersonToken::parse(structural_person_token(&ordinary)).is_ok());
    for (name, extra) in [
        (
            "unknown critical extension",
            json!({"crit":["unknown-extension"],"unknown-extension":true}),
        ),
        ("empty critical list", json!({"crit":[]})),
        (
            "unsupported unencoded payload",
            json!({"crit":["b64"],"b64":false}),
        ),
    ] {
        let mut header = ordinary.clone();
        header
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(
            ParsedPersonToken::parse(structural_person_token(&header)).is_err(),
            "person reader accepted {name}"
        );
    }
}
