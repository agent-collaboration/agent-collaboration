use crate::CallerKind;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[error("object call edge refused: {from:?} -> {to:?}")]
pub struct CallEdgeRefused {
    pub from: CallerKind,
    pub to: CallerKind,
}
pub fn validate_call_edge(from: CallerKind, to: CallerKind) -> Result<(), CallEdgeRefused> {
    if from.may_call(to) {
        Ok(())
    } else {
        Err(CallEdgeRefused { from, to })
    }
}
