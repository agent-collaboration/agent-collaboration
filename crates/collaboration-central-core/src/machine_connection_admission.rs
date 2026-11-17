//! The organization turn owns read-only connection admission and restoration.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    identity_contract::MachineConnectionRefusal,
    machine_connection::{Admission, MachineConnectionAttachment, validate_live, validate_shape},
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
