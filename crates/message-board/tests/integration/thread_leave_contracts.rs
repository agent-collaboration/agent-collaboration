use message_board::{BoardOperation, ThreadId, ThreadLeaveRequest};
use serde_json::json;

#[test]
fn leaving_uses_one_closed_follow_up_instead_of_conflicting_flags() {
    let base = json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,
        "thread":{"thread":ThreadId::generate()},
        "actor":{"kind":"human","humanId":"reader"},"afterLeave":null});
    for follow_up in [
        json!(null),
        json!({"kind":"handTo","to":{"kind":"human","humanId":"successor"}}),
        json!({"kind":"resolve","reason":"discussion concluded"}),
    ] {
        let mut request = base.clone();
        request["afterLeave"] = follow_up;
        assert!(serde_json::from_value::<ThreadLeaveRequest>(request).is_ok());
    }
    let mut old = base.clone();
    old["resolve"] = json!(true);
    assert!(serde_json::from_value::<ThreadLeaveRequest>(old).is_err());
    for follow_up in [
        json!({"kind":"resolve"}),
        json!({"kind":"resolve","reason":"  "}),
        json!({"kind":"resolve","reason":"done","to":{"kind":"human","humanId":"successor"}}),
    ] {
        let mut request = base.clone();
        request["afterLeave"] = follow_up;
        assert!(serde_json::from_value::<ThreadLeaveRequest>(request).is_err());
    }
}

#[test]
fn direct_resolve_and_leave_resolve_use_the_same_nonempty_reason() {
    for reason in ["", " \n\t "] {
        let operation =
            json!({"kind":"threadResolve","threadId":ThreadId::generate(),"reason":reason});
        assert!(serde_json::from_value::<BoardOperation>(operation).is_err());
    }
}
