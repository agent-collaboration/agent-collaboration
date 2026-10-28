use message_board::Refusal;
use serde_json::json;

#[test]
fn deferred_claim_retains_the_human_and_explicit_replacement_list() {
    let value = json!({"kind":"deferred","recipient":{
        "actor":{"kind":"human","humanId":"owner"},
        "viaRole":null},"replacements":[]});
    let decoded = serde_json::from_value::<message_board::IntentClaim>(value.clone());
    assert!(decoded.is_ok(), "deferred claim: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), value);
}

#[test]
fn no_holder_is_a_closed_role_refusal_that_can_be_stored_in_the_ledger() {
    let reason = json!({"kind":"role","reason":{"kind":"noHolder",
        "subject":{"kind":"project","subject":"01890f2e-7b4c-7cc0-98c4-000000000001"},
        "role":"lead"}});
    let decoded = serde_json::from_value::<Refusal>(reason.clone());
    assert!(decoded.is_ok(), "no-holder refusal: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), reason);
    let mut fallback = reason;
    fallback["reason"]["fallback"] = json!({"kind":"human","humanId":"someone"});
    assert!(serde_json::from_value::<Refusal>(fallback).is_err());
}

fn intent_record() -> serde_json::Value {
    json!({"envelope":{
        "id":"01890f2e-7b4c-7cc0-98c4-000000000010",
        "scope":{"workspace":"01890f2e-7b4c-7cc0-98c4-000000000002",
            "board":null,"project":"01890f2e-7b4c-7cc0-98c4-000000000001"},
        "actor":{"kind":"human","humanId":"sender"},
        "request":null,"position":{"kind":"project","position":{
            "cursor":{"epoch":1,"position":10},"origin":{
                "installation":"00000000-0000-4000-8000-000000000001","machine":"sender machine"}}},
        "time":{"kind":"router","value":"2026-10-12T00:00:00Z"}},
        "intentId":"01890f2e-7b4c-7cc0-98c4-000000000010",
        "cause":{"kind":"project","id":"01890f2e-7b4c-7cc0-98c4-000000000001"},
        "recipient":{"kind":"noHolder","recipient":{
            "subject":{"kind":"project","subject":"01890f2e-7b4c-7cc0-98c4-000000000001"},
            "role":"lead","assignmentRevision":0}},
        "state":{"kind":"refused","reason":{"kind":"noHolder"}}})
}

#[test]
fn refused_unfilled_role_records_no_fabricated_actor_and_cannot_be_emitted() {
    let input = intent_record();
    let decoded = serde_json::from_value::<message_board::NotifyIntent>(input.clone());
    assert!(decoded.is_ok(), "unfilled-role intent: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), input);
    let mut invalid = input;
    invalid["state"] = json!({"kind":"emitted"});
    assert!(serde_json::from_value::<message_board::NotifyIntent>(invalid).is_err());
}

#[test]
fn external_delivery_refusal_keeps_the_resolved_recipient_and_detail() {
    let mut input = intent_record();
    input["recipient"] = json!({"kind":"resolved","recipient":{
        "actor":input["envelope"]["actor"].clone(),"viaRole":null}});
    input["state"] =
        json!({"kind":"refused","reason":{"kind":"delivery","detail":"delivery declined"}});
    let decoded = serde_json::from_value::<message_board::NotifyIntent>(input.clone());
    assert!(decoded.is_ok(), "delivery refusal: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), input);
}
