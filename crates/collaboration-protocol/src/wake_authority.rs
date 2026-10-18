//! Declared wake creators are a confusion guard, not authenticated identities.
use crate::SessionRef;
use agent_automation::WakeupId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WakeCreator {
    Session(SessionRef),
    Owner {},
    Router {},
    Unrecorded {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WakeRefusal {
    WakeExists {
        wakeup: WakeupId,
        creator: WakeCreator,
    },
    TooFarAhead {
        max: u32,
    },
    IntervalTooShort {
        min: u32,
    },
    LifetimeTooLong {
        max: u32,
    },
    ExpiryRequired {},
}
