/// Test-support counts of whole notice-unit runs (V2 control gate).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyUnitInvocations {
    /// The due batch ran the notice-window unit.
    pub windows: u64,
    /// The due batch ran the workspace-drain unit.
    pub drains: u64,
    /// A request-triggered drain of one workspace ran (P5 change 5).
    pub request_drains: u64,
}
