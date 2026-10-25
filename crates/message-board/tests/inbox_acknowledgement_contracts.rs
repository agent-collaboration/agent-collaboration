use message_board::{InboxAcknowledgeRequest, ProjectId, RequestId};
use serde_json::json;

#[test]
fn aggregate_acknowledgement_keeps_each_scope_key_and_epoch_and_has_no_scalar_form() {
    let first = json!({"requestId":RequestId::generate(),
        "scope":{"kind":"project","projectId":ProjectId::generate()},"through":{"epoch":1,"position":41}});
    let second = json!({"requestId":RequestId::generate(),
        "scope":{"kind":"project","projectId":ProjectId::generate()},"through":{"epoch":2,"position":42}});
    let request = json!({"protocolVersion":1,"actor":{"kind":"human","humanId":"reader"},"actingFor":null,"entries":[first,second]});
    let parsed = serde_json::from_value::<InboxAcknowledgeRequest>(request.clone());
    assert!(parsed.is_ok(), "independent entries: {parsed:?}");
    assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), request);
    for entries in [json!([]), json!([first, first]), json!(vec![first; 101])] {
        let mut invalid = request.clone();
        invalid["entries"] = entries;
        assert!(serde_json::from_value::<InboxAcknowledgeRequest>(invalid).is_err());
    }
    let mut scalar = request;
    scalar["requestId"] = json!(RequestId::generate());
    scalar["throughActivitySequence"] = json!(42);
    assert!(serde_json::from_value::<InboxAcknowledgeRequest>(scalar).is_err());
}

#[test]
fn per_entry_reply_keeps_partial_outcomes_in_input_order() {
    let first = RequestId::generate();
    let second = RequestId::generate();
    let value = json!({"outcomes":[
        {"requestId":first,"outcome":{"kind":"replied","reply":{"result":{"kind":"decided","decision":{"kind":"accepted","effects":{"entries":[],"totalCount":0,"digest":"4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945","next":null},"change":"alreadySo"}},"reader":null}}},
        {"requestId":second,"outcome":{"kind":"replied","reply":{"result":{"kind":"leaseExpired","decision":{"expiredAt":"2026-10-12T00:00:00Z"}},"reader":null}}}
    ]});
    let decoded = serde_json::from_value::<message_board::InboxAcknowledgeReply>(value.clone());
    assert!(decoded.is_ok(), "partial reply: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), value);
}

#[test]
fn an_unanswered_entry_never_claims_that_the_project_did_not_apply_it() {
    let reason = serde_json::json!({"kind":"boardUnavailable","stage":"storage","message":"No answer from project","nextAction":"retryLater","details":{"kind":"none"}});
    let unanswered = serde_json::json!({"kind":"unanswered","reason":reason});
    let decoded =
        serde_json::from_value::<message_board::ProjectWriteCallResult>(unanswered.clone());
    assert!(decoded.is_ok(), "{decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), unanswered);
    assert!(
        serde_json::from_value::<message_board::ProjectWriteCallResult>(
            serde_json::json!({"kind":"failed","reason":reason})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<message_board::ProjectWriteCallResult>(
            serde_json::json!({"kind":"unanswered","reason":reason,"applied":false})
        )
        .is_err()
    );
}

#[test]
#[allow(clippy::panic_in_result_fn)]
fn acknowledgement_scopes_are_distinct_by_kind_and_id_before_routing()
-> Result<(), Box<dyn std::error::Error>> {
    use message_board::{InboxAcknowledgeRequestProps, ThreadId};
    let thread = ThreadId::generate();
    let project = ProjectId::generate();
    let qualified = json!({"kind":"thread","thread":{"thread":thread,"project":project}});
    let bare = json!({"kind":"thread","thread":{"thread":thread}});
    let differently_qualified =
        json!({"kind":"thread","thread":{"thread":thread,"project":ProjectId::generate()}});
    let request = |scopes: Vec<serde_json::Value>| json!({"protocolVersion":1,"actor":{"kind":"human","humanId":"scope contract"},"actingFor":null,"entries":scopes.into_iter().map(|scope| json!({"requestId":RequestId::generate(),"scope":scope,"through":{"epoch":1,"position":4}})).collect::<Vec<_>>()});
    for scopes in [
        vec![bare.clone(), qualified.clone()],
        vec![qualified.clone(), differently_qualified],
    ] {
        let value = request(scopes);
        let props: InboxAcknowledgeRequestProps = serde_json::from_value(value.clone())?;
        let error = InboxAcknowledgeRequest::new(props)
            .err()
            .ok_or("constructor admitted duplicate thread identity")?;
        assert_eq!(error.field, "entries");
        assert!(error.requirement.contains("distinct scopes"));
        assert!(
            serde_json::from_value::<InboxAcknowledgeRequest>(value).is_err(),
            "Serde bypassed semantic distinctness"
        );
    }
    for scopes in [
        vec![
            qualified,
            json!({"kind":"thread","thread":{"thread":ThreadId::generate(),"project":project}}),
        ],
        vec![bare, json!({"kind":"project","projectId":thread.as_str()})],
    ] {
        let value = request(scopes);
        let props: InboxAcknowledgeRequestProps = serde_json::from_value(value.clone())?;
        assert!(InboxAcknowledgeRequest::new(props).is_ok());
        assert!(
            serde_json::from_value::<InboxAcknowledgeRequest>(value).is_ok(),
            "distinct kinds or thread identities refused"
        );
    }
    Ok(())
}
