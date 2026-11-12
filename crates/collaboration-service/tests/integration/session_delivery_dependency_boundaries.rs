//! Feature modules request delivery through the seam, never a concrete client route.

#[test]
fn message_features_do_not_import_client_routes() {
    let features = [(
        "interaction notice",
        include_str!("../../../collaboration-runtime/src/interaction_broker.rs"),
    )];
    for (feature, source) in features {
        for forbidden in [
            "native_message_dispatch",
            "codex_app_server_delivery_route",
            "provider_acp_delivery_route",
            "claude_code_peer_delivery_route",
            "codex_native_integration",
            "NativeOperation",
            "codex/messageSend",
            "codex-local",
            "claude-local",
            "cursor-local",
        ] {
            assert!(
                !source.contains(forbidden),
                "{feature} depends on {forbidden}"
            );
        }
    }
}
