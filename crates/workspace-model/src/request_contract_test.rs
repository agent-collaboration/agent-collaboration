use super::WorkspaceOperation;

#[test]
fn owner_close_backlog_and_self_join_roles_are_admitted_semantic_inputs() {
    for input in [
        serde_json::json!({"kind":"closeBacklogItem","itemId":"01890f2e-7b4c-7cc0-98c4-000000000001","note":"Delivered","link":null}),
        serde_json::json!({"kind":"joinThread","threadId":"01890f2e-7b4c-7cc0-98c4-000000000002","role":"observer"}),
        serde_json::json!({"kind":"joinThread","threadId":"01890f2e-7b4c-7cc0-98c4-000000000002","role":"participant"}),
    ] {
        let operation: WorkspaceOperation = serde_json::from_value(input.clone())
            .expect("owner-approved semantic operation must decode");
        assert_eq!(serde_json::to_value(operation).unwrap(), input);
    }
}

#[test]
fn workspace_name_collision_preserves_the_typed_name_in_its_wire_refusal() {
    let name = crate::ResourceName::try_from("Research".to_owned()).unwrap();
    let refusal = crate::RefusalReason::WorkspaceNameInUse { name };
    let expected = serde_json::json!({"kind":"workspaceNameInUse","name":"Research"});

    assert_eq!(serde_json::to_value(&refusal).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<crate::RefusalReason>(expected).unwrap(),
        refusal
    );
    assert!(
        serde_json::from_value::<crate::RefusalReason>(
            serde_json::json!({"kind":"workspaceNameInUse","name":""})
        )
        .is_err()
    );
}
