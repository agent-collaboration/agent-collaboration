//! Person proof admission and read operations on modern org central.
use crate::{
    AdminRefusal, ObjectHost, OrganizationAdminAnswer, OrganizationAdminResult, WorkspaceRuntime,
    WorkspaceStoreCalls,
    organization_authority_cache::StampStanding,
    person_admission_plans::prepare_person_admission,
    person_request::{AdminOperation, PersonRequestInput, PersonRequestRefusal},
};
use central_storage::{BatchOutcome, StatementBatch, StoreFailure};
impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    pub async fn admin_person_request(
        &self,
        input: &PersonRequestInput,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        // An unstamped org reveals nothing about a presented person proof.
        {
            let _turn = self.take_central_turn().await;
            self.prepare_signin_authority().await?;
            if self.signin_authority.standing(self.clock.now()) != StampStanding::Valid {
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
        if verified.organization_id != self.organization {
            return Ok(OrganizationAdminAnswer::Unauthenticated);
        }
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::BeforeSignInSample)
            .await;
        let _turn = self.take_central_turn().await;
        self.prepare_signin_authority().await?;
        let now = self.now()?;
        if self
            .signin_authority
            .standing(runtime_primitives::SyncTime::from_unix_millis(
                now.timestamp_millis(),
            ))
            != StampStanding::Valid
        {
            return Err(StoreFailure::Unavailable);
        }
        let plan = match prepare_person_admission(self.host.as_ref(), &verified, now).await? {
            Ok(plan) => plan,
            Err(reason) => {
                return Ok(OrganizationAdminAnswer::Answered(
                    OrganizationAdminResult::Refused {
                        reason: reason.into(),
                    },
                ));
            }
        };
        let mut statements = plan.statements;
        let mut operation = None;
        let result = match &verified.operation {
            AdminOperation::CreateWorkspace(request) => {
                let created = self
                    .prepare_workspace_create(&verified, request, now)
                    .await?;
                statements.extend(created.statements);
                operation = Some(created.operation);
                created.result
            }
            AdminOperation::WorkspaceLookup(request) => {
                let workspace = request
                    .workspace_id
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| crate::workspace_decode_error::corrupt("workspace_id"))?;
                let found =
                    crate::workspace_directory_rows::read_by_id(self.host.as_ref(), &workspace)
                        .await?
                        .map(crate::organization_workspace_plans::workspace_view)
                        .transpose()?;
                OrganizationAdminResult::Found { workspace: found }
            }
            AdminOperation::ReadChangeRecords(request) => OrganizationAdminResult::Records {
                records: crate::organization_change_rows::read_page(
                    self.host.as_ref(),
                    request.after,
                    request.limit.get(),
                )
                .await?,
            },
        };
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::AfterSignInSample)
            .await;
        match self
            .host
            .run_batch(StatementBatch {
                statements,
                alarm: central_storage::AlarmUpdate::Unchanged,
            })
            .await?
        {
            BatchOutcome::Applied { .. } => {
                // Reuse the existing operation-id loss hook after a known commit;
                // no record-only API or fake committed answer is introduced.
                #[cfg(feature = "test-support")]
                if operation
                    .as_ref()
                    .is_some_and(|id| self.take_lost_reply(id))
                {
                    return Err(StoreFailure::AnswerLost);
                }
                #[cfg(not(feature = "test-support"))]
                let _retained_operation = operation;
                Ok(OrganizationAdminAnswer::Answered(result))
            }
            _ => Ok(OrganizationAdminAnswer::Answered(
                OrganizationAdminResult::Refused {
                    reason: AdminRefusal::Conflict,
                },
            )),
        }
    }
}
