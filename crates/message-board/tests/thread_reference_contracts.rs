//! Command targets carry optional project routing; stored conversation relations keep IDs.
use message_board::*;
use serde_json::json;

#[test]
fn thread_show_accepts_only_the_new_thread_reference_target() {
    let thread = ThreadId::from(PostId::generate());
    let project = ProjectId::generate();
    for target in [
        json!({"thread":thread}),
        json!({"thread":thread,"project":project}),
    ] {
        let value = json!({"thread":target,"reader":{"kind":"anonymous"},"includeRedacted":false});
        let decoded = serde_json::from_value::<ThreadShowRequest>(value.clone());
        assert!(
            decoded.is_ok(),
            "valid ThreadRef target refused: {decoded:?}"
        );
        assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), value);
    }
    assert!(
        serde_json::from_value::<ThreadShowRequest>(
            json!({"threadId":thread,"reader":{"kind":"anonymous"}})
        )
        .is_err(),
        "the scalar target is removed at the public cutover"
    );
    assert!(
        serde_json::from_value::<ThreadShowRequest>(
            json!({"thread":{"thread":thread,"project":""},"reader":{"kind":"anonymous"}})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ThreadShowRequest>(
            json!({"thread":{"thread":thread,"unexpected":true},"reader":{"kind":"anonymous"}})
        )
        .is_err()
    );
}

#[test]
fn a_thread_post_target_is_separate_from_stored_placement_and_membership_scope() {
    let thread = ThreadId::from(PostId::generate());
    let project = ProjectId::generate();
    let input = json!({"requestId":RequestId::generate(),"protocolVersion":1,
        "postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread,"project":project}},
        "actor":{"kind":"human","humanId":"writer"},"actingFor":null,"text":"qualified target","references":[],"addressees":[]});
    assert!(serde_json::from_value::<ThreadPostRequest>(input).is_ok());
    let stored = json!({"kind":"thread","threadId":thread});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<Placement>(stored.clone()).unwrap()).unwrap(),
        stored
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<SubscriptionScope>(stored.clone()).unwrap())
            .unwrap(),
        stored
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<ReadScope>(stored.clone()).unwrap()).unwrap(),
        stored
    );
}
