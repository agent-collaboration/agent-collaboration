use serde::{Deserialize, Serialize};

/// Independently supported additions to the released single-frame protocol.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameCapabilities {
    pub batches: bool,
    pub notices_applied: bool,
}

impl FrameCapabilities {
    pub const SUPPORTED: Self = Self {
        batches: true,
        notices_applied: true,
    };

    pub const fn intersection(self, other: Self) -> Self {
        Self {
            batches: self.batches && other.batches,
            notices_applied: self.notices_applied && other.notices_applied,
        }
    }

    pub const fn contains(self, other: Self) -> bool {
        (!other.batches || self.batches) && (!other.notices_applied || self.notices_applied)
    }
}
