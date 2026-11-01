//! The owner-only Unix transport carries a caller claim, never a binding capability.
use crate::SessionRef;
use serde::{Deserialize, Serialize};

pub const UNIX_SESSION_CONTEXT_HEADER: &str = "x-codex-router-session-context";

/// The Host resolves this claim to an active local registration. Same-user
/// processes can name any local session; this is not caller authentication.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnixSessionContext {
    pub session: SessionRef,
}
