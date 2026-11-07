//! The session picker's machine list, read from real files at a Router root
//! through the public reader: `agent-sessions.jsonc` only, never `routers.jsonc`.

use agent_session_inventory::sessions::{
    RouterRegistryRead, router_connection_registry::read_router_registry,
};
use std::path::Path;

fn write_machine_list(root: &Path, file_name: &str, machine_name: &str, service_id: &str) {
    let document = serde_json::json!({"version": 1, "routers": [{
        "name": machine_name,
        "connection": {"kind": "remote", "serviceId": service_id,
            "mcpUrl": "https://machine.example.invalid:443/mcp"}
    }]});
    std::fs::write(root.join(file_name), document.to_string()).expect("machine list fixture");
}

fn write_new_list(root: &Path) {
    write_machine_list(
        root,
        "agent-sessions.jsonc",
        "New machine",
        "00000000-0000-4000-8000-000000000002",
    );
}

fn write_old_list(root: &Path) {
    write_machine_list(
        root,
        "routers.jsonc",
        "Old machine",
        "00000000-0000-4000-8000-000000000001",
    );
}

/// The text the picker shows for a rejected list (picker_machine_controls.rs).
fn picker_notice(read: RouterRegistryRead) -> String {
    match read {
        RouterRegistryRead::Rejected(error) => error.to_string(),
        other => panic!("expected the list to be refused, got {other:?}"),
    }
}

#[test]
fn new_machine_list_alone_lists_exactly_its_machines() {
    let root = tempfile::tempdir().expect("router root");
    write_new_list(root.path());

    let read = read_router_registry(root.path());

    let RouterRegistryRead::Ready(registry) = read else {
        panic!("agent-sessions.jsonc must be read: {read:?}")
    };
    let names: Vec<&str> = registry
        .routers
        .iter()
        .map(|profile| profile.name.as_str())
        .collect();
    assert_eq!(names, vec!["New machine"]);
}

#[test]
fn old_machine_list_alone_lists_no_machines_and_names_the_update() {
    let root = tempfile::tempdir().expect("router root");
    write_old_list(root.path());

    let notice = picker_notice(read_router_registry(root.path()));

    assert!(notice.contains("routers.jsonc"), "{notice}");
    assert!(notice.contains("agent-router-update"), "{notice}");
    assert!(!notice.contains("Old machine"), "{notice}");
}

#[test]
fn both_machine_lists_use_neither_and_name_both() {
    let root = tempfile::tempdir().expect("router root");
    write_old_list(root.path());
    write_new_list(root.path());

    let notice = picker_notice(read_router_registry(root.path()));

    assert!(notice.contains("routers.jsonc"), "{notice}");
    assert!(notice.contains("agent-sessions.jsonc"), "{notice}");
    assert_eq!(
        std::fs::read_dir(root.path())
            .expect("root listing")
            .count(),
        2,
        "reading never moves or removes either file"
    );
}
