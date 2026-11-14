//! Person proof admission and read operations on modern org central.
use crate::{
    AdminRefusal, ObjectHost, OrganizationAdminAnswer, OrganizationAdminResult, WorkspaceRuntime,
    WorkspaceStoreCalls,
    organization_authority_cache::StampStanding,
    person_admission_plans::prepare_person_admission,
    person_request::{AdminOperation, PersonRequestInput, PersonRequestRefusal},
};
use central_storage::{BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure};
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
        let result = match &verified.operation {
            AdminOperation::CreateWorkspace(_) => return Err(StoreFailure::Unavailable), // Owner-held mapping is never a fabricated success.
            AdminOperation::WorkspaceLookup(request) => {
                let rows = self
                    .host
                    .read(Statement {
                        sql: "SELECT workspace_id FROM workspaces WHERE workspace_id=?",
                        args: vec![SqlValue::Text(request.workspace_id.as_str().to_owned())],
                    })
                    .await?;
                // The existing-workspace representation is held with create-workspace.
                if !rows.rows.is_empty() {
                    return Err(StoreFailure::Unavailable);
                }
                OrganizationAdminResult::Found { workspace: None }
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
                statements: plan.statements,
                alarm: central_storage::AlarmUpdate::Unchanged,
            })
            .await?
        {
            BatchOutcome::Applied { .. } => Ok(OrganizationAdminAnswer::Answered(result)),
            _ => Ok(OrganizationAdminAnswer::Answered(
                OrganizationAdminResult::Refused {
                    reason: AdminRefusal::Conflict,
                },
            )),
        }
    }
}
