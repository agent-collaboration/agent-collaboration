//! The organization turn owns read-only connection admission and restoration.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    identity_contract::MachineConnectionRefusal,
    machine_connection::{
        Admission, MachineConnectionAttachment, ValidatedMachineConnection, validate_live,
        validate_shape,
    },
    organization_authority::machine_login::{EnrollmentFacts, EnrollmentQuery},
};
use central_storage::StoreFailure;
use fleet_identity_protocol::{EnrollmentId, InstallationId};

impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
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
        let _turn = self.take_central_turn_for_admission().await;
        if verified.organization_id() != &self.organization {
            return Ok(Err(
                MachineConnectionRefusal::MachineInAnotherOrganization {},
            ));
        }
        // Row identity is resolved from the public installation, never supplied by the caller.
        let time = self.machine_login_time(verified.organization_id()).await?;
        let facts = self
            .connection_facts(verified.installation_id(), verified.enrollment_id())
            .await?;
        let Some(machine) = &facts.machine else {
            return Ok(Err(MachineConnectionRefusal::UnknownMachine {}));
        };
        let attachment = MachineConnectionAttachment::from_verified(&verified, machine.id);
        validate_live(&attachment, &self.organization, &facts, time.now_ms)
    }

    /// Renews an open connection's machine token (fleet-access R40/V32, B3):
    /// "On the open connection, the Router sends the fresh token
    /// (`RenewMachineToken`), and central answers `renewed` or
    /// `refused { reason }`." The token is verified exactly as at connect, then
    /// the enrollment is read live under the central turn. It must name the
    /// connection's own organization, installation, enrollment and client. The
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
        let _turn = self.take_central_turn_for_admission().await;
        if verified.organization_id() != current.organization_id()
            || verified.installation_id() != current.installation_id()
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
            .connection_facts(verified.installation_id(), verified.enrollment_id())
            .await?;
        let Some(machine) = &facts.machine else {
            return Ok(Err(Refusal::EnrollmentNotActive));
        };
        let renewed = MachineConnectionAttachment::from_verified(&verified, machine.id);
        Ok(
            match validate_live(&renewed, &self.organization, &facts, time.now_ms)? {
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
        let _turn = self.take_central_turn_for_admission().await;
        self.restore_machine_connection_held(attachment).await
    }

    /// Caller holds the existing central turn through the decision or effect it admits.
    pub(crate) async fn restore_machine_connection_held(
        &self,
        attachment: &MachineConnectionAttachment,
    ) -> Admission {
        if attachment.organization_id() != &self.organization {
            return Ok(Err(
                MachineConnectionRefusal::MachineInAnotherOrganization {},
            ));
        }
        validate_shape(attachment)?;
        let time = self
            .machine_login_time(attachment.organization_id())
            .await?;
        let facts = self
            .connection_facts(attachment.installation_id(), attachment.enrollment_id())
            .await?;
        validate_live(attachment, &self.organization, &facts, time.now_ms)
    }

    async fn connection_facts(
        &self,
        installation: &InstallationId,
        enrollment: &EnrollmentId,
    ) -> Result<EnrollmentFacts, StoreFailure> {
        crate::machine_login_reads::facts(
            self.host.as_ref(),
            self.organization.clone(),
            &EnrollmentQuery {
                installation: Some(installation.clone()),
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
    /// It names another organization or installation than the connection.
    DifferentMachine,
    /// It names another enrollment or client than the connection.
    DifferentEnrollment,
}

type Renewal = Result<Result<ValidatedMachineConnection, MachineTokenRenewalRefusal>, StoreFailure>;
