use super::{AreaId, ProjectId, WorkspaceId};

#[test]
fn canonical_v7_is_required_on_construction_and_decode() {
    let good = "01890f2e-7b4c-7cc0-98c4-000000000001";
    assert!(good.parse::<AreaId>().is_ok());
    for invalid in [
        "",
        "01890f2e-7b4c-4cc0-98c4-000000000001",
        "01890f2e-7b4c-7cc0-08c4-000000000001",
        "01890F2E-7B4C-7CC0-98C4-000000000001",
        "01890f2e7b4c7cc098c4000000000001",
    ] {
        assert!(invalid.parse::<AreaId>().is_err());
        assert!(serde_json::from_str::<ProjectId>(&format!("\"{invalid}\"")).is_err());
    }
    assert!(serde_json::from_str::<WorkspaceId>(&format!("\"{good}\"")).is_ok());
}
