use message_board::{BoardOperation, LifecycleFact, ThreadId};
use serde_json::json;

#[test]
fn resolving_a_thread_requires_a_reason_in_its_operation() {
    let thread = ThreadId::generate();
    let request = json!({"kind":"threadResolve","threadId":thread,"reason":"discussion concluded"});
    assert!(serde_json::from_value::<BoardOperation>(request).is_ok());
    assert!(
        serde_json::from_value::<BoardOperation>(json!({"kind":"threadResolve","threadId":thread}))
            .is_err()
    );
}
#[test]
fn resolution_fact_names_the_thread_and_keeps_its_reason() {
    let input = json!({"kind":"threadResolved","thread":ThreadId::generate(),"reason":"discussion concluded"});
    let decoded = serde_json::from_value::<LifecycleFact>(input.clone());
    assert!(
        decoded.is_ok(),
        "missing thread lifecycle fact: {decoded:?}"
    );
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), input);
}
#[test]
fn unresolve_is_separate_and_accepts_no_closing_reason() {
    let thread = ThreadId::generate();
    assert!(
        serde_json::from_value::<BoardOperation>(
            json!({"kind":"threadUnresolve","threadId":thread})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<BoardOperation>(
            json!({"kind":"threadUnresolve","threadId":thread,"reason":"unused"})
        )
        .is_err()
    );
}
