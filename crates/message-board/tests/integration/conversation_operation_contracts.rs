use message_board::{BoardOperation, ProjectId, ThreadId};
use serde_json::json;

#[test]
fn conversation_and_membership_writes_enter_the_common_project_request() {
    let thread = ThreadId::generate();
    for operation in [
        json!({"kind":"threadJoin","threadId":thread,"role":"reviewer","watch":true,"mode":null,"whenIdle":null,"replace":null,"note":null}),
        json!({"kind":"threadLeave","threadId":thread,"afterLeave":{"kind":"resolve","reason":"complete"}}),
        json!({"kind":"threadWatch","threadId":thread}),
        json!({"kind":"threadUnwatch","threadId":thread}),
        json!({"kind":"projectWatch"}),
        json!({"kind":"projectUnwatch"}),
        json!({"kind":"threadSubscribe","scope":{"kind":"project","projectId":ProjectId::generate()},"policy":{"mode":null,"whenIdle":null,"quietSeconds":null,"capSeconds":null,"lifetimeSeconds":null}}),
        json!({"kind":"threadUnsubscribe","scope":{"kind":"thread","threadId":thread}}),
        json!({"kind":"threadRenew","scope":{"kind":"thread","threadId":thread}}),
    ] {
        let parsed = serde_json::from_value::<BoardOperation>(operation.clone());
        assert!(
            parsed.is_ok(),
            "missing declared operation {operation}: {parsed:?}"
        );
        assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), operation);
    }
}
