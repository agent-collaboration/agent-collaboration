use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The closed failure classes of engine_work_attempts.last_failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineFailureClass {
    CorruptRow,
    ConstraintViolated,
    StatementRejected,
    Refused,
    OtherStoreUnavailable,
}
