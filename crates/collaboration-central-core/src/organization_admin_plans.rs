//! Person proof admission and read operations on modern org central.
use crate::{
    AdminRefusal, MachineEnrollmentRevocationRefusal, MachineEnrollmentRevocationResult,
    OrganizationAdminAnswer, OrganizationAdminResult, WorkspaceRuntime,
    organization_authority_cache::StampStanding,
    person_admission_plans::prepare_person_admission,
    person_request::{AdminOperation, AdmissionRefusal, PersonRequestInput, PersonRequestRefusal},
};
use central_storage::{BatchOutcome, StatementBatch, StoreFailure};
use fleet_contracts::WorkspaceDirectory;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_workspace_directory::CentralWorkspaceDirectory;
impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    pub async fn admin_person_request(
        &self,
        input: &PersonRequestInput,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        // An unstamped org reveals nothing about a presented person proof.
        {
            let _turn = self.execution.take_central_turn().await;
            self.prepare_signin_authority().await?;
            if self.signin_authority.standing(self.execution.clock().now()) != StampStanding::Valid
            {
                return Err(StoreFailure::Unavailable);
            }
        }
        let check = self
            .signin_authority
            .check()
            .ok_or(StoreFailure::Unavailable)?;
        if check.sign_in_service().is_none() {
            return Err(StoreFailure::Unavailable);
        }
        let verified = match check.verify_person_request(input).await {
            Ok(verified) => verified,
            Err(PersonRequestRefusal::VerificationUnavailable | PersonRequestRefusal::Policy) => {
                return Err(StoreFailure::Unavailable);
            }
            Err(_) => return Ok(OrganizationAdminAnswer::Unauthenticated),
        };
        if &verified.organization_id != self.execution.organization() {
            return Ok(OrganizationAdminAnswer::Unauthenticated);
        }
        #[cfg(feature = "test-support")]
        self.execution
            .pause_points()
            .reached_central(fleet_runtime::PausePoint::BeforeSignInSample)
            .await;
        let _turn = self.execution.take_central_turn().await;
        self.prepare_signin_authority().await?;
        let now = self.execution.now()?;
        if self
            .signin_authority
            .standing(fleet_runtime_ports::SyncTime::from_unix_millis(
                now.timestamp_millis(),
            ))
            != StampStanding::Valid
        {
            return Err(StoreFailure::Unavailable);
        }
        let plan = match prepare_person_admission(self.execution.statement_host(), &verified, now)
            .await?
        {
            Ok(plan) => plan,
            Err(reason) => {
                if matches!(
                    verified.operation,
                    AdminOperation::RevokeMachineEnrollment(_)
                ) {
                    return Ok(revocation_admission_refusal(reason));
                }
                return Ok(OrganizationAdminAnswer::Answered(
                    OrganizationAdminResult::Refused {
                        reason: reason.into(),
                    },
                ));
            }
        };
        let mut statements = plan.statements;
        let mut operation = None;
        let mut retained = None;
        let mut alarm = central_storage::AlarmUpdate::Unchanged;
        let mut guard_failed = AdminRefusal::Conflict;
        let result = match &verified.operation {
            AdminOperation::RevokeMachineEnrollment(revocation) => {
                return self
                    .revoke_machine_enrollment_from_admin(
                        &verified,
                        &revocation.request.enrollment_id,
                        statements,
                        now.timestamp_millis(),
                    )
                    .await;
            }
            AdminOperation::CreateWorkspace(request) => {
                let created = self
                    .prepare_workspace_create(&verified, request, now)
                    .await?;
                statements.extend(created.statements);
                operation = Some(created.operation);
                retained = created.retained;
                created.result
            }
            AdminOperation::WorkspaceLookup(request) => {
                let workspace = request
                    .workspace_id
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| {
                        fleet_runtime_ports::workspace_decode_error::corrupt("workspace_id")
                    })?;
                let directory = CentralWorkspaceDirectory::new(self.execution.statement_host());
                let found = directory
                    .read_by_id(&workspace)
                    .await?
                    .map(|row| directory.workspace_view(row))
                    .transpose()?;
                OrganizationAdminResult::Found { workspace: found }
            }
            AdminOperation::ReadChangeRecords(request) => OrganizationAdminResult::Records {
                records: crate::organization_change_rows::read_page(
                    self.execution.statement_host(),
                    request.after,
                    request.limit.get(),
                )
                .await?,
            },
            AdminOperation::EngineHealth(_) => self.engine_health_result().await?.result,
            AdminOperation::EngineListRetired(request) => {
                self.engine_retired_result(request.after, request.limit.get())
                    .await?
                    .result
            }
            AdminOperation::EngineRetry(request) => {
                let planned = self.prepare_engine_retry(&verified, request, now).await?;
                statements.extend(planned.statements);
                alarm = planned.alarm;
                retained = planned.retained;
                // A concurrent retry deleted the record first.
                guard_failed = AdminRefusal::NotFound;
                planned.result
            }
            AdminOperation::EngineDiscard(request) => {
                let planned = self
                    .prepare_engine_discard(&verified, request, &mut statements, now)
                    .await?;
                if planned.committed {
                    return Ok(OrganizationAdminAnswer::Answered(planned.result));
                }
                statements.extend(planned.statements);
                retained = planned.retained;
                planned.result
            }
        };
        #[cfg(feature = "test-support")]
        self.execution
            .pause_points()
            .reached_central(fleet_runtime::PausePoint::AfterSignInSample)
            .await;
        match self
            .execution
            .statement_host()
            .run_batch(StatementBatch { statements, alarm })
            .await?
        {
            BatchOutcome::Applied { .. } => {
                // Reuse the existing operation-id loss hook after a known commit;
                // no record-only API or fake committed answer is introduced.
                #[cfg(feature = "test-support")]
                if operation
                    .as_ref()
                    .is_some_and(|id| self.execution.take_lost_reply(id))
                {
                    return Err(StoreFailure::AnswerLost);
                }
                #[cfg(not(feature = "test-support"))]
                let _retained_operation = operation;
                Ok(match retained {
                    Some(answer) => OrganizationAdminAnswer::Retained(answer),
                    None => OrganizationAdminAnswer::Answered(result),
                })
            }
            // A rejected statement keeps main's store-outage answer on this path.
            BatchOutcome::StatementRejected { .. } => Err(StoreFailure::Unavailable),
            _ => Ok(OrganizationAdminAnswer::Answered(
                OrganizationAdminResult::Refused {
                    reason: guard_failed,
                },
            )),
        }
    }
}

fn revocation_admission_refusal(reason: AdmissionRefusal) -> OrganizationAdminAnswer {
    let reason = match reason {
        AdmissionRefusal::NotPermitted => MachineEnrollmentRevocationRefusal::NotPermitted,
        AdmissionRefusal::SecondFactorRequired => {
            MachineEnrollmentRevocationRefusal::FactorNotRecent
        }
        _ => return OrganizationAdminAnswer::Unauthenticated,
    };
    OrganizationAdminAnswer::MachineEnrollmentRevocation(
        MachineEnrollmentRevocationResult::Refused { reason },
    )
}
