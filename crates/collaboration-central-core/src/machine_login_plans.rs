//! One org turn owns machine-login reads, applying time and the guarded batch.
use crate::identity_contract::{
    DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision, MachineTokenDecision,
};
use crate::organization_authority::machine_login::{
    self, Decided, DecisionTime, EnrollmentFacts, EnrollmentQuery,
};
use crate::organization_authority_cache::StampStanding;
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::{AlarmUpdate, BatchOutcome, StatementBatch, StoreFailure};
use runtime_primitives::SyncTime;

impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    pub async fn decide_device_login(
        &self,
        request: &DecideDeviceLogin,
    ) -> Result<DeviceLoginDecision, StoreFailure> {
        let received_at_ms = self.clock.now().unix_millis();
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::BeforeSignInSample)
            .await;
        let _turn = self.take_central_turn().await;
        let time = self.machine_login_time(&request.organization_id).await?;
        let machine_id = request
            .installation
            .as_str()
            .parse()
            .map_err(|_| crate::workspace_decode_error::corrupt("issuer.installation"))?;
        let query = EnrollmentQuery {
            machine_id: Some(machine_id),
            request: Some(request.request_id.clone()),
            workspaces: request.allowed_workspaces.as_slice().to_vec(),
            ..EnrollmentQuery::default()
        };
        let facts = crate::machine_login_reads::facts(
            self.host.as_ref(),
            self.organization.clone(),
            &query,
        )
        .await?;
        let decided = machine_login::decide_device_login(
            request,
            &facts,
            received_at_ms,
            time,
            self.ids.as_ref(),
        )?;
        self.apply_machine_login(&facts, decided, time).await
    }

    pub async fn decide_machine_token(
        &self,
        request: &DecideMachineToken,
    ) -> Result<MachineTokenDecision, StoreFailure> {
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::BeforeSignInSample)
            .await;
        let _turn = self.take_central_turn().await;
        let time = self.machine_login_time(&request.organization_id).await?;
        let query = EnrollmentQuery {
            enrollment: Some(request.enrollment_id.clone()),
            ..EnrollmentQuery::default()
        };
        let facts = crate::machine_login_reads::facts(
            self.host.as_ref(),
            self.organization.clone(),
            &query,
        )
        .await?;
        let decided =
            machine_login::decide_machine_token(request, &facts, time, self.ids.as_ref())?;
        self.apply_machine_login(&facts, decided, time).await
    }

    pub(crate) async fn machine_login_time(
        &self,
        organization: &fleet_identity_protocol::OrganizationId,
    ) -> Result<DecisionTime, StoreFailure> {
        if organization != &self.organization {
            return Err(StoreFailure::Unavailable);
        }
        self.prepare_signin_authority().await?;
        let time = DecisionTime {
            now_ms: self.clock.now().unix_millis(),
        };
        if self
            .signin_authority
            .standing(SyncTime::from_unix_millis(time.now_ms))
            != StampStanding::Valid
            || self
                .signin_authority
                .check()
                .and_then(|check| check.sign_in_service())
                .is_none()
        {
            return Err(StoreFailure::Unavailable);
        }
        Ok(time)
    }

    pub(crate) async fn apply_machine_login<T>(
        &self,
        facts: &EnrollmentFacts,
        decided: Decided<T>,
        time: DecisionTime,
    ) -> Result<T, StoreFailure> {
        if match decided.written_at_ms {
            Some(at) => at != time.now_ms || decided.changes.is_empty(),
            None => !decided.changes.is_empty(),
        } {
            return Err(crate::workspace_decode_error::corrupt(
                "machine_login.applying_time",
            ));
        }
        let statements =
            crate::machine_login_writes::statements(facts, &decided.changes, time.now_ms)?;
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::AfterSignInSample)
            .await;
        if !statements.is_empty() {
            match self
                .host
                .run_batch(StatementBatch {
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await?
            {
                BatchOutcome::Applied { .. } => {}
                _ => return Err(StoreFailure::Unavailable),
            }
        }
        Ok(decided.answer)
    }
}
