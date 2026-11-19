//! Which permission mode a new or updated Session is put in (R3, R5).
//!
//! No requested mode means `auto`. `auto` maps to the agent's own mode that
//! realizes it; an agent's own mode name passes through; anything else is
//! refused with the accepted list. The plan is pure: the caller applies it
//! against the agent and reports the requested and actual modes.

/// The router-level name every agent accepts for its auto realization.
pub const AUTO_PERMISSION_MODE: &str = "auto";

/// How an agent's auto mode treats an action outside its granted folders (R7).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoRealization {
    /// The agent's classifier decides; a denial must be raised as an approval.
    ClassifierEscalates,
    /// The agent asks before acting outside its grant.
    AsksFirst,
    /// The agent's behaviour outside its grant has not been established.
    Unknown,
}

/// An agent's own answer to "what does auto mean for you", declared by its
/// `ProviderBehavior`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoPermissionMode {
    /// The agent's own mode that realizes auto, or `None` when the agent has
    /// no such mode and keeps whatever it starts in.
    pub realized_by: Option<&'static str>,
    /// The agent's ask-first mode used when `realized_by` is not on offer or
    /// the agent itself falls back to it.
    pub fallback: Option<&'static str>,
    pub realization: AutoRealization,
    /// Why `realized_by` is not the agent's own auto, when it is a stand-in.
    pub stand_in_reason: Option<PermissionModeReason>,
}

impl AutoPermissionMode {
    /// An agent the Router has no auto realization for: its starting mode stays.
    pub const UNREALIZED: Self = Self {
        realized_by: None,
        fallback: None,
        realization: AutoRealization::Unknown,
        stand_in_reason: None,
    };
}

/// What the caller asked for, after naming rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestedPermissionMode {
    /// No mode was given; R3 makes that auto.
    Unspecified,
    Auto,
    AgentMode(String),
}

impl RequestedPermissionMode {
    #[must_use]
    pub fn from_request(requested: Option<&str>) -> Self {
        match requested {
            None => Self::Unspecified,
            Some(AUTO_PERMISSION_MODE) => Self::Auto,
            Some(name) => Self::AgentMode(name.to_owned()),
        }
    }

    /// The name the caller sent, if any.
    #[must_use]
    pub fn requested_name(&self) -> Option<&str> {
        match self {
            Self::Unspecified => None,
            Self::Auto => Some(AUTO_PERMISSION_MODE),
            Self::AgentMode(name) => Some(name),
        }
    }
}

/// Why the actual mode differs from the one asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionModeReason {
    /// The agent does not offer its auto mode, so its ask-first mode is used.
    AutoNotOffered,
    /// The agent accepted auto but put the session in its ask-first mode.
    AgentFellBack,
    /// The Router has no auto realization for this agent; its mode is unchanged.
    AutoNotRealized,
    /// The agent's own auto refuses outside actions without asking, and its
    /// escalation is not proven, so its ask-first mode stands in (R6).
    EscalationUnproven,
    /// Setting the auto realization failed; the agent stays in its own mode.
    AutoNotApplied,
}

/// The mode to apply after the agent advertised its modes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionModePlan {
    /// Set the agent's mode to `mode`. `accepted_fallback` names the one other
    /// mode the agent may answer with (its own auto fallback) without failing.
    Set {
        requested: RequestedPermissionMode,
        mode: String,
        accepted_fallback: Option<String>,
        reason: Option<PermissionModeReason>,
    },
    /// Leave the agent in the mode it started in.
    Keep {
        requested: RequestedPermissionMode,
        reason: Option<PermissionModeReason>,
    },
}

/// A requested name that is neither `auto` nor one of the agent's own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionModeRefusal {
    pub value: String,
    /// `auto` followed by the agent's advertised modes.
    pub accepted: Vec<String>,
}

/// Builds the plan for one agent from the requested mode and the modes the
/// agent advertised for this session.
pub fn plan_permission_mode(
    requested: RequestedPermissionMode,
    advertised: &[String],
    auto: AutoPermissionMode,
) -> Result<PermissionModePlan, PermissionModeRefusal> {
    let offered = |name: &str| advertised.iter().any(|mode| mode == name);
    if let RequestedPermissionMode::AgentMode(name) = &requested {
        if offered(name) {
            let mode = name.clone();
            return Ok(PermissionModePlan::Set {
                requested,
                mode,
                accepted_fallback: None,
                reason: None,
            });
        }
        let mut accepted = vec![AUTO_PERMISSION_MODE.to_owned()];
        accepted.extend(
            advertised
                .iter()
                .filter(|mode| mode.as_str() != AUTO_PERMISSION_MODE)
                .cloned(),
        );
        return Err(PermissionModeRefusal {
            value: name.clone(),
            accepted,
        });
    }
    let fallback = auto.fallback.filter(|mode| offered(mode));
    match (auto.realized_by.filter(|mode| offered(mode)), fallback) {
        (Some(mode), fallback) => Ok(PermissionModePlan::Set {
            requested,
            mode: mode.to_owned(),
            accepted_fallback: fallback.map(str::to_owned),
            reason: auto.stand_in_reason,
        }),
        (None, Some(fallback)) => Ok(PermissionModePlan::Set {
            requested,
            mode: fallback.to_owned(),
            accepted_fallback: None,
            reason: Some(PermissionModeReason::AutoNotOffered),
        }),
        (None, None) => Ok(PermissionModePlan::Keep {
            requested,
            reason: Some(if auto.realized_by.is_some() {
                PermissionModeReason::AutoNotOffered
            } else {
                PermissionModeReason::AutoNotRealized
            }),
        }),
    }
}

/// The requested and actual mode of a session and why they differ (R3).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PermissionModeReport {
    pub requested: Option<String>,
    pub actual: Option<String>,
    pub reason: Option<PermissionModeReason>,
}

/// Why the optional mode portion of a restore differs from its saved setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoredModeDifference {
    NotOffered,
    Refused,
    NoAnswer,
    EarlierWriteUnanswered,
    DifferentReply,
    UnconfirmedReply,
}

#[cfg(test)]
#[path = "permission_mode_plan_test.rs"]
mod tests;
