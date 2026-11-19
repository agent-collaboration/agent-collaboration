use super::{RequestScope, RequestStatusAnswer, RequestStatusQuery};
use crate::RequestPayload;
use serde_json::json;

const REQUEST: &str = "019f0000-0000-7000-8000-000000000001";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000002";

#[test]
fn a_request_status_query_selects_one_store() {
    for scope in [
        json!({"kind":"organization"}),
        json!({"kind":"workspace","workspaceId":WORKSPACE}),
    ] {
        let wire = json!({"requestId":REQUEST,"scope":scope});
        let query: RequestStatusQuery = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(query).unwrap(), wire);
    }
    for scope in [
        json!({"kind":"workspace"}),
        json!({"kind":"organization","workspaceId":WORKSPACE}),
        json!({"kind":"allWorkspaces"}),
    ] {
        let wire = json!({"requestId":REQUEST,"scope":scope});
        assert!(serde_json::from_value::<RequestStatusQuery>(wire).is_err());
    }
}

#[test]
fn a_workspace_directory_write_keeps_its_organization_scope() {
    let request = |operation| -> RequestPayload {
        serde_json::from_value(json!({
            "kind":"workspace",
            "request":{"requestId":REQUEST,"workspaceId":WORKSPACE,"operation":operation}
        }))
        .unwrap()
    };
    let directory = request(json!({"kind":"archiveWorkspace"}));
    let workspace = request(json!({"kind":"archiveArea","areaId":WORKSPACE}));
    assert_eq!(directory.request_scope(), RequestScope::Organization);
    assert_eq!(
        workspace.request_scope(),
        RequestScope::Workspace(WORKSPACE.parse().unwrap())
    );
}

#[test]
fn replay_answers_keep_the_logged_reply_and_domain_refusal_shapes() {
    let cases = [
        json!({"kind":"answered","value":{"kind":"mail","reply":{"kind":"done","value":{"kind":"queued","messageId":REQUEST}}}}),
        json!({"kind":"refused","value":{"kind":"workspace","reason":{"kind":"workspaceNotAllowed"}}}),
        json!({"kind":"refused","value":{"kind":"mail","reason":{"kind":"notSession"}}}),
        json!({"kind":"notFound"}),
    ];
    for wire in cases {
        let answer: RequestStatusAnswer = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(answer).unwrap(), wire);
    }
    let rejected = json!({"kind":"refused","value":{"kind":"workspace","reason":{"kind":"credentialRefused"}}});
    assert!(serde_json::from_value::<RequestStatusAnswer>(rejected).is_err());
}

#[test]
fn message_status_shapes_refuse_extra_fields_and_open_reasons() {
    let query: workspace_model::ListQuery =
        serde_json::from_value(json!({"kind":"messageStatus","messageId":REQUEST})).unwrap();
    assert!(matches!(
        query,
        workspace_model::ListQuery::MessageStatus { .. }
    ));
    use workspace_model::central_mail::{DeliveryAnswer, MessageFailureReason, MessageStatus};
    for (reason, wire_reason) in [
        (MessageFailureReason::UnknownSession, "unknownSession"),
        (MessageFailureReason::AmbiguousSession, "ambiguousSession"),
        (MessageFailureReason::SenderUnresolved, "senderUnresolved"),
    ] {
        let failed = MessageStatus::Failed { reason };
        let wire = json!({"kind":"failed","reason":wire_reason});
        assert_eq!(serde_json::to_value(failed).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<MessageStatus>(wire).unwrap(),
            failed
        );
    }
    assert!(
        serde_json::from_value::<DeliveryAnswer>(
            json!({"kind":"refused","reason":"senderUnresolved"})
        )
        .is_err()
    );
    for wire in [
        json!({"kind":"failed","reason":"arbitrary"}),
        json!({"kind":"failed","reason":"recipientRefused"}),
        json!({"kind":"queued","reason":"unknownSession"}),
        json!({"kind":"delivered","messageId":REQUEST}),
        json!({"kind":"notFound","messageId":REQUEST}),
    ] {
        assert!(serde_json::from_value::<MessageStatus>(wire).is_err());
    }
}
