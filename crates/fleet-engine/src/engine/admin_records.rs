use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The closed set of engine kinds an admin sees; test-support machines are hidden.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineKindName {
    WakeTimer,
    AutomationTimer,
    LegacyNoticeWindows,
    LegacyWorkspaceDrains,
    EngineSource,
}

/// The closed failure classes of central_work_attempts.last_failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineFailureClass {
    CorruptRow,
    ConstraintViolated,
    StatementRejected,
    Refused,
    OtherStoreUnavailable,
}
