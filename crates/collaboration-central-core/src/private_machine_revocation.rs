//! Apply a verified revocation and its fresh-proof reservation in one org turn.
use crate::{
    MachineEnrollmentRevocationResult as ResultKind, OrganizationAdminAnswer, WorkspaceRuntime,
    identity_contract::PersonId,
    organization_authority::machine_login::{
        self, DecisionTime, EnrollmentQuery, StoredEnrollmentState,
    },
    person_request::{AdminOperationId, VerifiedPersonRequest},
    workspace_decode_error::corrupt,
};
use central_storage::{AlarmUpdate, BatchOutcome, GuardedStatement, StatementBatch, StoreFailure};
use fleet_identity_protocol::EnrollmentId;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    /// The caller holds the central turn through admission, commit and notification.
    pub(crate) async fn revoke_machine_enrollment_from_admin(
        &self,
        request: &VerifiedPersonRequest,
        enrollment_id: &EnrollmentId,
        mut statements: Vec<GuardedStatement>,
        now_ms: i64,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        let facts = crate::machine_login_reads::facts(
            self.host.as_ref(),
            self.organization.clone(),
            &EnrollmentQuery {
                enrollment: Some(enrollment_id.clone()),
                ..EnrollmentQuery::default()
            },
        )
        .await?;
        let (result, revoked_machine) = match &facts.enrollment {
            None => (ResultKind::NotFound {}, None),
            Some(enrollment) => match &enrollment.state {
                StoredEnrollmentState::Approved => (ResultKind::NotFound {}, None),
                StoredEnrollmentState::Lapsed
                | StoredEnrollmentState::Revoked { .. }
                | StoredEnrollmentState::Expired { .. } => (ResultKind::AlreadyEnded {}, None),
                StoredEnrollmentState::Enrolled { .. } => {
                    if facts.active.as_ref().map(|active| &active.id) != Some(enrollment_id) {
                        return Err(corrupt("login_machine_approvals.enrolled"));
                    }
                    let actor: PersonId = request
                        .person_id
                        .clone()
                        .try_into()
                        .map_err(|_| corrupt("person_request.person_id"))?;
                    // The proof ID correlates this one audit effect. The wire
                    // operation has no operationId and retries use a fresh proof.
                    let audit_request = AdminOperationId::try_from(request.request_id.to_string())
                        .map_err(|_| corrupt("person_request.request_id"))?;
                    let revocation = machine_login::revoke_machine_enrollment(
                        &facts,
                        &actor,
                        &audit_request,
                        request.operation.route().permission(),
                        DecisionTime { now_ms },
                        self.ids.as_ref(),
                    )?;
                    if revocation.answer.is_none() {
                        return Err(corrupt("login_machine_approvals.enrolled"));
                    }
                    statements.extend(crate::machine_login_writes::statements(
                        &facts,
                        &revocation.changes,
                        now_ms,
                    )?);
                    (ResultKind::Revoked {}, Some(enrollment.machine_id.clone()))
                }
            },
        };
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::AfterSignInSample)
            .await;
        if !matches!(
            self.host
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await?,
            BatchOutcome::Applied { .. }
        ) {
            return Err(StoreFailure::Unavailable);
        }
        if let Some(machine_id) = revoked_machine {
            (self.machine_revoked)(machine_id);
        }
        Ok(OrganizationAdminAnswer::MachineEnrollmentRevocation(result))
    }
}
