use super::*;
use crate::{DataFrame, FrameDirection, LinkFrame, ProtocolError, decode_text, encode_text};
use serde_json::json;

const CORRELATION: &str = "019f0000-0000-7000-8000-0000000c0001";

fn renew_json(token: &str) -> String {
    json!({"kind": "renewMachineToken", "correlationId": CORRELATION, "token": token}).to_string()
}

#[test]
fn renew_and_its_answer_have_the_approved_wire_shape_and_directions() {
    let renew = decode_text(
        &renew_json("header.claims.signature"),
        FrameDirection::ToCentral,
    )
    .unwrap();
    let renewed = json!({
        "kind": "machineTokenRenewal",
        "correlationId": CORRELATION,
        "result": {"kind": "renewed", "effectiveExpiresAt": "2030-03-17T18:46:40Z"},
    })
    .to_string();
    let refused = json!({
        "kind": "machineTokenRenewal",
        "correlationId": CORRELATION,
        "result": {"kind": "refused", "reason": "enrollmentNotActive"},
    })
    .to_string();

    let renewed = decode_text(&renewed, FrameDirection::ToRouter).unwrap();
    let refused = decode_text(&refused, FrameDirection::ToRouter).unwrap();

    let LinkFrame::Data(DataFrame::RenewMachineToken { token, .. }) = &renew else {
        panic!("renew frame: {renew:?}");
    };
    assert_eq!(token.expose(), "header.claims.signature");
    assert!(matches!(
        renewed,
        LinkFrame::Data(DataFrame::MachineTokenRenewal {
            result: TokenRenewalResult::Renewed { .. },
            ..
        })
    ));
    assert!(matches!(
        refused,
        LinkFrame::Data(DataFrame::MachineTokenRenewal {
            result: TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::EnrollmentNotActive
            },
            ..
        })
    ));
    assert!(matches!(
        decode_text(&renew_json("a.b.c"), FrameDirection::ToRouter),
        Err(ProtocolError::WrongDirection)
    ));
    assert!(matches!(
        encode_text(&renewed, FrameDirection::ToCentral),
        Err(ProtocolError::WrongDirection)
    ));
}

#[test]
fn every_refusal_reason_has_its_camel_case_wire_name() {
    let names = [
        (TokenRenewalRefusal::TokenRejected, "tokenRejected"),
        (
            TokenRenewalRefusal::EnrollmentNotActive,
            "enrollmentNotActive",
        ),
        (TokenRenewalRefusal::DifferentMachine, "differentMachine"),
        (
            TokenRenewalRefusal::DifferentEnrollment,
            "differentEnrollment",
        ),
        (
            TokenRenewalRefusal::CentralUnavailable,
            "centralUnavailable",
        ),
    ];

    for (reason, name) in names {
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(name));
    }
}

#[test]
fn the_token_is_never_shown_and_must_be_one_compact_value() {
    let token = MachineTokenText::try_from("secret.token.value".to_owned()).unwrap();
    let frame = LinkFrame::Data(DataFrame::RenewMachineToken {
        correlation_id: CORRELATION.parse().unwrap(),
        token: token.clone(),
    });

    assert!(!format!("{token:?}").contains("secret"));
    assert!(!format!("{frame:?}").contains("secret"));
    assert!(MachineTokenText::try_from(String::new()).is_err());
    assert!(MachineTokenText::try_from("a b".to_owned()).is_err());
    assert!(decode_text(&renew_json(""), FrameDirection::ToCentral).is_err());
    let extra = json!({"kind": "renewMachineToken", "correlationId": CORRELATION, "token": "a.b.c", "extra": 1}).to_string();
    assert!(decode_text(&extra, FrameDirection::ToCentral).is_err());
}
