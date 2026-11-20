//! The organization turn owns read-only connection admission and restoration.
use crate::{
    WorkspaceRuntime,
    identity_contract::MachineConnectionRefusal,
    machine_connection::{
        Admission, MachineConnectionAttachment, ValidatedMachineConnection, validate_live,
        validate_shape,
    },
    organization_authority::machine_login::{EnrollmentFacts, EnrollmentQuery},
};
use central_storage::StoreFailure;
use fleet_identity_protocol::EnrollmentId;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_ids::MachineId;

/// Live authority for one ordinary link request or one member of a write batch.
pub enum MachineLinkRequestAuthority {
    Admitted(ValidatedMachineConnection),
    Refused,
    TemporarilyUnavailable,
}

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    pub async fn restore_machine_link_request(
        &self,
        attachment: &MachineConnectionAttachment,
    ) -> Result<MachineLinkRequestAuthority, StoreFailure> {
        match self.restore_machine_connection(attachment).await {
            Ok(Ok(connection)) => Ok(MachineLinkRequestAuthority::Admitted(connection)),
            Ok(Err(_)) => Ok(MachineLinkRequestAuthority::Refused),
            Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
                Ok(MachineLinkRequestAuthority::TemporarilyUnavailable)
            }
            Err(
                error @ (StoreFailure::Corrupt { .. } | StoreFailure::AlarmUnpublishable { .. }),
            ) => Err(error),
        }
    }

    pub async fn admit_machine_connection(&self, token: &str) -> Admission {
        let check = self
            .signin_authority
            .check()
            .ok_or(StoreFailure::Unavailable)?;
        let verified = match check.verify_machine_token(token).await {
            Ok(verified) => verified,
            Err(crate::machine_token::MachineTokenRefusal::Rejected) => {
                return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
            }
            Err(crate::machine_token::MachineTokenRefusal::Unavailable) => {
                return Err(StoreFailure::Unavailable);
            }
        };
        let _turn = self.execution.take_central_turn_for_admission().await;
        if verified.organization_id() != self.execution.organization() {
            return Ok(Err(
                MachineConnectionRefusal::MachineInAnotherOrganization {},
            ));
        }
        let time = self.machine_login_time(verified.organization_id()).await?;
        let facts = self
            .connection_facts(verified.machine_id(), verified.enrollment_id())
            .await?;
        let attachment = MachineConnectionAttachment::from_verified(&verified);
        validate_live(
            &attachment,
            self.execution.organization(),
            &facts,
            time.now_ms,
        )
    }

    /// Renews an open connection's machine token (fleet-access R40/V32, B3):
    /// "On the open connection, the Router sends the fresh token
    /// (`RenewMachineToken`), and central answers `renewed` or
    /// `refused { reason }`." The token is verified exactly as at connect, then
    /// the enrollment is read live under the central turn. It must name the
    /// connection's own organization, machine_id, enrollment and client. The
    /// renewed connection counts until the new token's
    /// min(exp + 60 s, grant_expires_at), which may extend it; on refusal the
    /// caller keeps the current connection and its expiry.
    ///
    /// A renewal is a frame on the current connection, so it is refused like any
    /// frame once that connection's effective deadline has passed (no leeway,
    /// refused at equality) or its enrollment is no longer live, with the same
    /// EnrollmentNotActive answer on both hosts; the Router reconnects with a
    /// fresh token. The deadline is checked before the token and again under the
    /// turn, since verification may wait across it.
    pub async fn renew_machine_connection(
        &self,
        current: &ValidatedMachineConnection,
        token: &str,
    ) -> Renewal {
        use MachineTokenRenewalRefusal as Refusal;
        match self
            .restore_machine_connection(&current.attachment())
            .await?
        {
            Ok(_) => {}
            Err(_) => return Ok(Err(Refusal::EnrollmentNotActive)),
        }
        let check = self
            .signin_authority
            .check()
            .ok_or(StoreFailure::Unavailable)?;
        let verified = match check.verify_machine_token(token).await {
            Ok(verified) => verified,
            Err(crate::machine_token::MachineTokenRefusal::Rejected) => {
                return Ok(Err(Refusal::TokenRejected));
            }
            Err(crate::machine_token::MachineTokenRefusal::Unavailable) => {
                return Err(StoreFailure::Unavailable);
            }
        };
        let _turn = self.execution.take_central_turn_for_admission().await;
        if verified.organization_id() != current.organization_id()
            || verified.machine_id() != current.machine_id()
        {
            return Ok(Err(Refusal::DifferentMachine));
        }
        if verified.enrollment_id() != current.enrollment_id()
            || verified.client_id() != current.client_id()
        {
            return Ok(Err(Refusal::DifferentEnrollment));
        }
        let time = self.machine_login_time(verified.organization_id()).await?;
        if time.now_ms.div_euclid(1000) >= current.effective_expires_at().0 {
            return Ok(Err(Refusal::EnrollmentNotActive));
        }
        let facts = self
            .connection_facts(verified.machine_id(), verified.enrollment_id())
            .await?;
        if facts.machine.is_none() {
            return Ok(Err(Refusal::EnrollmentNotActive));
        }
        let renewed = MachineConnectionAttachment::from_verified(&verified);
        Ok(
            match validate_live(&renewed, self.execution.organization(), &facts, time.now_ms)? {
                Ok(connection) => Ok(connection),
                Err(MachineConnectionRefusal::WrongClient {}) => Err(Refusal::DifferentEnrollment),
                Err(
                    MachineConnectionRefusal::UnknownMachine {}
                    | MachineConnectionRefusal::EnrollmentNotActive {}
                    | MachineConnectionRefusal::Superseded {}
                    | MachineConnectionRefusal::MachineInAnotherOrganization {},
                ) => Err(Refusal::EnrollmentNotActive),
            },
        )
    }

    pub async fn restore_machine_connection(
        &self,
        attachment: &MachineConnectionAttachment,
    ) -> Admission {
        let _turn = self.execution.take_central_turn_for_admission().await;
        #[cfg(feature = "test-support")]
        if let Some(failure) = self.take_machine_restore_failure() {
            return Err(failure);
        }
        self.restore_machine_connection_held(attachment).await
    }

    /// Fail one restore after the specified number of successful fixture restores.
    #[cfg(feature = "test-support")]
    pub fn fail_machine_restore_after(&self, skip: usize, failure: StoreFailure) {
        *self
            .machine_restore_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((skip, failure));
    }

    #[cfg(feature = "test-support")]
    fn take_machine_restore_failure(&self) -> Option<StoreFailure> {
        let mut next = self
            .machine_restore_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match next.as_mut() {
            Some((remaining, _)) if *remaining > 0 => {
                *remaining -= 1;
                None
            }
            _ => next.take().map(|(_, failure)| failure),
        }
    }

    /// Register socket ownership while the live authority check still holds
    /// the organization turn. Revocation then either sees the registration or
    /// wins the turn first and refuses it.
    pub async fn register_active_machine<Value>(
        &self,
        attachment: &MachineConnectionAttachment,
        register: impl FnOnce() -> Value,
    ) -> Result<Result<Value, MachineConnectionRefusal>, StoreFailure> {
        let _turn = self.execution.take_central_turn_for_admission().await;
        Ok(self
            .restore_machine_connection_held(attachment)
            .await?
            .map(|_| register()))
    }

    /// Caller holds the existing central turn through the decision or effect it admits.
    pub(crate) async fn restore_machine_connection_held(
        &self,
        attachment: &MachineConnectionAttachment,
    ) -> Admission {
        if attachment.organization_id() != self.execution.organization() {
            return Ok(Err(
                MachineConnectionRefusal::MachineInAnotherOrganization {},
            ));
        }
        validate_shape(attachment)?;
        let time = self
            .machine_login_time(attachment.organization_id())
            .await?;
        let facts = self
            .connection_facts(attachment.machine_id(), attachment.enrollment_id())
            .await?;
        validate_live(
            attachment,
            self.execution.organization(),
            &facts,
            time.now_ms,
        )
    }

    async fn connection_facts(
        &self,
        machine_id: &MachineId,
        enrollment: &EnrollmentId,
    ) -> Result<EnrollmentFacts, StoreFailure> {
        crate::machine_login_reads::facts(
            self.execution.statement_host(),
            self.execution.organization().clone(),
            &EnrollmentQuery {
                machine_id: Some(machine_id.clone()),
                enrollment: Some(enrollment.clone()),
                ..EnrollmentQuery::default()
            },
        )
        .await
    }
}

/// Why central refused to renew an open connection's machine token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineTokenRenewalRefusal {
    /// The token failed sign-in verification.
    TokenRejected,
    /// Its enrollment is not the machine's active one, or its grant has run out.
    EnrollmentNotActive,
    /// It names another organization or machine_id than the connection.
    DifferentMachine,
    /// It names another enrollment or client than the connection.
    DifferentEnrollment,
}

type Renewal = Result<Result<ValidatedMachineConnection, MachineTokenRenewalRefusal>, StoreFailure>;
