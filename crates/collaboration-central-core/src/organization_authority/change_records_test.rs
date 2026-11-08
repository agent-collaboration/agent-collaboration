//! The stored kind string, the parser and the wire name agree for every kind.
use super::OrganizationChangeKind;

#[test]
fn every_change_kind_is_stored_parsed_and_sent_under_one_name() {
    let kinds = [
        (
            OrganizationChangeKind::OrganizationProvisioned,
            "organizationProvisioned",
        ),
        (OrganizationChangeKind::StampRenewed, "stampRenewed"),
        (OrganizationChangeKind::WorkspaceCreated, "workspaceCreated"),
        (OrganizationChangeKind::MachineEnrolled, "machineEnrolled"),
        (OrganizationChangeKind::MachineRevoked, "machineRevoked"),
        (OrganizationChangeKind::AllowanceChanged, "allowanceChanged"),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.as_str(), name);
        assert_eq!(OrganizationChangeKind::parse(name).ok(), Some(kind));
        assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(name));
    }
    assert!(OrganizationChangeKind::parse("machineExpired").is_err());
}
