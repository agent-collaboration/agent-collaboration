//! The one injected clock and the UUIDv7 id source that reads it.
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// UTC wall-clock time in milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SyncTime(i64);

impl SyncTime {
    pub const fn from_unix_millis(milliseconds: i64) -> Self {
        Self(milliseconds)
    }

    pub const fn unix_millis(self) -> i64 {
        self.0
    }

    /// Saturates at the latest representable time rather than wrapping a deadline.
    pub fn after(self, delay: Duration) -> Self {
        Self(
            self.0
                .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX)),
        )
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> SyncTime;
}

pub trait IdSource: Send + Sync {
    /// Mint a UUIDv7 once at the owning boundary.
    fn new_id(&self) -> Uuid;
}

/// True for an RFC 4122 version-7 UUID.
pub fn uuid_v7(value: Uuid) -> bool {
    value.get_version_num() == 7 && value.get_variant() == uuid::Variant::RFC4122
}

/// Text must be checked before parsing can normalize its spelling.
pub fn canonical_uuid_v7(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| uuid_v7(id) && id.hyphenated().to_string() == value)
}

#[cfg(test)]
#[path = "time_and_ids_test.rs"]
mod tests;
