//! Directional conflicts between one admission and an existing operation.

use collaboration_protocol::{OperationId, ProviderOperationKind, SessionRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DispatchMark {
    BeforeHandoff,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdmissionClass {
    Independent,
    LinePlacement,
    RefuseWhileRunning { foreground: uuid::Uuid },
}

#[derive(Clone, Copy)]
pub(super) struct AdmissionMode {
    pub(super) mark: DispatchMark,
    pub(super) class: AdmissionClass,
    pub(super) cancel_target: Option<uuid::Uuid>,
}

impl AdmissionMode {
    pub(super) const HANDOFF: Self = Self {
        mark: DispatchMark::BeforeHandoff,
        class: AdmissionClass::Independent,
        cancel_target: None,
    };

    pub(super) const fn deferred(class: AdmissionClass) -> Self {
        Self {
            mark: DispatchMark::Deferred,
            class,
            cancel_target: None,
        }
    }

    pub(super) fn canceling(target: &OperationId) -> Self {
        Self {
            cancel_target: uuid::Uuid::parse_str(target.as_str()).ok(),
            ..Self::HANDOFF
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct AdmissionRequest<'a> {
    pub(super) operation: &'a OperationId,
    pub(super) kind: ProviderOperationKind,
    pub(super) target: Option<&'a SessionRef>,
    pub(super) mode: AdmissionMode,
}

pub(super) enum EarlierOperation<'a> {
    Unknown,
    InFlight { target: Option<&'a SessionRef> },
}

impl AdmissionRequest<'_> {
    /// Only a lane mark leaves the exact cancel and its input to the owner.
    pub(super) fn is_own_cancel_holder(
        self,
        holder_kind: Option<ProviderOperationKind>,
        target: Option<uuid::Uuid>,
    ) -> bool {
        holder_kind == Some(ProviderOperationKind::ConversationCancel)
            && target.is_some()
            && uuid::Uuid::parse_str(self.operation.as_str()).ok() == target
    }
    pub(super) fn checks_blockers(self) -> bool {
        self.kind != ProviderOperationKind::ConversationCancel
    }

    pub(super) fn blocked_by(
        self,
        earlier: &OperationId,
        evidence: EarlierOperation<'_>,
        unconfirmed_cancel: Option<&str>,
    ) -> bool {
        use ProviderOperationKind::ConversationClose;

        if !self.checks_blockers() {
            return false;
        }
        match evidence {
            // Close may recover its own unconfirmed foreground cancel.
            EarlierOperation::Unknown => {
                self.kind != ConversationClose || unconfirmed_cancel != Some(earlier.as_str())
            }
            // Close can interrupt live work. Line placement belongs
            // to the owner, and same-ID reuse consults the existing operation.
            EarlierOperation::InFlight { .. }
                if self.kind == ConversationClose
                    || self.mode.class == AdmissionClass::LinePlacement
                    || self.operation == earlier =>
            {
                false
            }
            EarlierOperation::InFlight { target: None } => true,
            EarlierOperation::InFlight { target: Some(held) } => {
                self.target == Some(held)
                    && !matches!(self.mode.class, AdmissionClass::RefuseWhileRunning { foreground }
                        if uuid::Uuid::parse_str(earlier.as_str()).ok() == Some(foreground))
            }
        }
    }
}

/// The two blocker types retain their existing caller guidance.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AdmissionBlockers {
    pub(super) unknown: Vec<OperationId>,
    pub(super) in_flight: Vec<OperationId>,
}

impl AdmissionBlockers {
    pub(super) fn answer(&self) -> Option<String> {
        let ids = |operations: &[OperationId]| {
            operations
                .iter()
                .map(OperationId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut clauses = Vec::new();
        if !self.unknown.is_empty() {
            clauses.push(format!(
                "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
                ids(&self.unknown)
            ));
        }
        if !self.in_flight.is_empty() {
            clauses.push(format!(
                "an earlier send is still in flight; wait for {}",
                ids(&self.in_flight)
            ));
        }
        (!clauses.is_empty()).then(|| clauses.join("; "))
    }
}

#[cfg(test)]
#[path = "provider_admission_policy_test.rs"]
mod tests;
