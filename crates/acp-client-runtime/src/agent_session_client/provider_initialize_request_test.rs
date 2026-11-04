use super::required_authentication_request;
use crate::ProviderInitializationAuthentication::{NotRequested, RequiredAgentMethod};
use serde_json::{Value, json};

/// Oracle: locked ACP v1 Agent-method semantics, including absent type;
/// Cursor's documented cursor_login method is never selected by offer order.
#[test]
fn initialization_authentication_selects_the_exact_typed_agent_offer() {
    for explicit_type in [false, true] {
        // Arrange: unrelated malformed/future offers do not broaden our validation.
        let mut selected = json!({"id":"cursor_login","name":"Cursor login"});
        if explicit_type {
            selected["type"] = json!("agent");
        }
        let response = json!({"authMethods":[
            null, {"id":"other","type":"future"}, selected, 9
        ]});

        // Act / Assert: one exact advertised Agent request, with no guessed ID.
        let request = required_authentication_request(
            &response,
            RequiredAgentMethod {
                method_id: "cursor_login",
            },
        )
        .expect("qualified offer")
        .expect("required request");
        assert_eq!(
            serde_json::to_value(request).expect("request"),
            json!({"methodId":"cursor_login"})
        );
    }
}

#[test]
fn initialization_authentication_rejects_missing_ambiguous_and_malformed_selected_offers() {
    let valid = json!({"id":"cursor_login","name":"Cursor login"});
    let mut cases = vec![
        Value::Null,
        json!({}),
        json!({"authMethods":null}),
        json!({"authMethods":{}}),
        json!({"authMethods":"cursor_login"}),
        json!({"authMethods":[]}),
        json!({"authMethods":[{"id":"other","name":"Other login"}]}),
        json!({"authMethods":[valid,valid]}),
        // SDK VecSkipError would discard the malformed duplicate's missing name.
        json!({"authMethods":[valid,{"id":"cursor_login"}]}),
        json!({"authMethods":[{"id":"cursor_login","name":5}]}),
        json!({"authMethods":[{"id":"cursor_login"}]}),
    ];
    for kind in [
        json!("terminal"),
        json!("future"),
        Value::Null,
        json!(4),
        json!(false),
    ] {
        let mut selected = valid.clone();
        selected["type"] = kind;
        cases.push(json!({"authMethods":[selected]}));
    }
    for response in cases {
        assert!(
            required_authentication_request(
                &response,
                RequiredAgentMethod {
                    method_id: "cursor_login"
                },
            )
            .is_err(),
            "unqualified selected offer: {response}"
        );
    }
}

#[test]
fn initialization_authentication_not_requested_ignores_unselected_auth_offers() {
    for response in [Value::Null, json!({"authMethods":"malformed"}), json!({})] {
        assert!(
            required_authentication_request(&response, NotRequested)
                .expect("unchanged policy")
                .is_none()
        );
    }
}
