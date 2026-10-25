//! An edge dispatch binds its typed destination before serialization.
use crate::{ObjectCallId, ObjectKind};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "", rename_all = "camelCase", deny_unknown_fields)]
pub struct Dispatch<To: ObjectKind> {
    pub call_id: ObjectCallId,
    pub to: To::Address,
    pub call: To::Call,
}
