use message_board::{Refusal, RequestDecision, ThreadId};
use serde_json::json;

#[test]
fn seat_refusals_remain_distinct_and_replayable_with_holder_evidence() {
    let thread = ThreadId::generate();
    let actor = json!({"kind":"human","humanId":"requester"});
    let holder = json!({"kind":"human","humanId":"current-holder"});
    let reason = json!({"kind":"thread","reason":{"kind":"staleOrchestrator","threadId":thread,"actor":actor,"holder":holder,"lastSeen":{"epoch":2,"position":42},"namedHolder":{"kind":"human","humanId":"stale-holder"}}});
    let decision = json!({"kind":"rejected","reasons":[reason]});
    let parsed: Result<RequestDecision, _> = serde_json::from_value(decision.clone());
    assert!(
        parsed.is_ok(),
        "thread refusal cannot enter the ledger: {parsed:?}"
    );
    assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), decision);
}
#[test]
fn stale_named_holder_can_report_a_vacant_seat_without_inventing_a_cursor() {
    let reason = json!({"kind":"thread","reason":{"kind":"staleImplementer","threadId":ThreadId::generate(),"actor":{"kind":"human","humanId":"requester"},"holder":null,"lastSeen":null,"namedHolder":{"kind":"human","humanId":"former-holder"}}});
    assert!(serde_json::from_value::<Refusal>(reason.clone()).is_ok());
    let mut invalid = reason;
    invalid["reason"]["lastSeen"] = json!({"epoch":1,"position":0});
    assert!(serde_json::from_value::<Refusal>(invalid).is_err());
}
#[test]
fn thread_refusals_are_closed_and_cannot_be_authority_role_refusals() {
    let base = json!({"kind":"thread","reason":{"kind":"selfReplace","threadId":ThreadId::generate(),"actor":{"kind":"human","humanId":"requester"}}});
    assert!(serde_json::from_value::<Refusal>(base.clone()).is_ok());
    let mut extra = base.clone();
    extra["reason"]["unknown"] = json!(true);
    assert!(serde_json::from_value::<Refusal>(extra).is_err());
    let mut wrong = base;
    wrong["reason"]["kind"] = json!("roleRequired");
    assert!(serde_json::from_value::<Refusal>(wrong).is_err());
}
