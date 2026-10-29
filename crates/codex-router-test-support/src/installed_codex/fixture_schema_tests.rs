//! A legacy synchronous seed must be ready for the real read-only quota command.
use super::hostile_and_state::migrate_fixture_state_and_capture_quota_status;
use codex_router_state::sqlite::SqliteStateStore;

#[test]
fn seeded_fixture_is_upgraded_before_read_only_quota_views() -> crate::FixtureResult {
    let root = tempfile::tempdir()?;
    let state_path = root.path().join("state.sqlite");
    let _legacy = SqliteStateStore::open(&state_path)?;
    let captured = migrate_fixture_state_and_capture_quota_status(&state_path)?;
    let report: serde_json::Value = serde_json::from_str(&captured.json)?;
    if report.get("accounts") != Some(&serde_json::json!([])) {
        return Err(
            "empty fixture did not preserve its exact account set through quota rendering".into(),
        );
    }
    Ok(())
}
