//! Verified stamp decisions become static guarded batches under the org turn.
use crate::identity_contract::{
    AuthorizationRevision, AuthorizationRevisionAcknowledgement, OrganizationStatementDigestReply,
    ProvisionOrganization, ProvisionOrganizationResult, ReadOrganizationStatementDigest,
};
use crate::organization_authority::OrganizationProvisioningFailure;
use crate::organization_authority::{
    CurrentStamp, FleetOrganizationStatementRefusal, OrganizationStatementReplacement,
    OrganizationStatementReplacementRefusal, StoredStampRead,
    decide_organization_statement_replacement,
};
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    organization_authority_cache::StampStanding,
    organization_change_rows::{
        self, OrganizationChangeKind, OrganizationChangeOrigin, OrganizationChangeRecord,
    },
    organization_stamp_rows,
};
use central_storage::{BatchOutcome, StatementBatch, StoreFailure};
use runtime_primitives::SyncTime;
impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    pub(crate) async fn prepare_signin_authority(&self) -> Result<(), StoreFailure> {
        let authority = &self.signin_authority;
        if authority.needs_check() {
            let stored = organization_stamp_rows::read(self.host.as_ref()).await?;
            if let StoredStampRead::Stamped(stamp) = &stored
                && stamp.organization_id() != &self.organization
            {
                return Err(crate::workspace_decode_error::corrupt(
                    "organization_identity.id",
                ));
            }
            let verified = match &stored {
                StoredStampRead::Unprovisioned => None,
                StoredStampRead::Stamped(stamp) => {
                    let check = authority.check().ok_or(StoreFailure::Unavailable)?;
                    Some(check.verify_statement(stamp.statement()).await)
                }
            };
            authority.record(
                &stored,
                verified
                    .as_ref()
                    .map(|result| result.as_ref().map_err(|reason| *reason)),
            );
        }
        Ok(())
    }
    pub async fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        use OrganizationProvisioningFailure as Failure;
        if request.organization_id != self.organization {
            return Err(Failure::OrganizationMismatch);
        }
        {
            let _turn = self.take_central_turn().await;
            self.prepare_signin_authority().await?;
        }
        let check = self.signin_authority.check().ok_or(Failure::Unavailable)?;
        let verified = check
            .verify_statement(request.organization_statement.as_str())
            .await
            .map_err(|reason| {
                (self.signin_observer)(crate::CentralObservation::OrganizationStatementRefused {
                    organization: self.organization.clone(),
                    refusal: reason,
                });
                match reason {
                    FleetOrganizationStatementRefusal::OrganizationMismatch => {
                        Failure::OrganizationMismatch
                    }
                    FleetOrganizationStatementRefusal::VerificationUnavailable(_)
                    | FleetOrganizationStatementRefusal::InvalidConfiguration => {
                        Failure::Unavailable
                    }
                    _ => Failure::InvalidStatement,
                }
            })?;
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::BeforeSignInSample)
            .await;
        let _turn = self.take_central_turn().await;
        self.prepare_signin_authority().await?;
        let now = self.clock.now();
        verified.require_valid_at(now).map_err(|refusal| {
            (self.signin_observer)(crate::CentralObservation::OrganizationStatementRefused {
                organization: self.organization.clone(),
                refusal,
            });
            Failure::InvalidStatement
        })?;
        let stored_trusted = self.signin_authority.standing(now) != StampStanding::Refused;
        let stored = organization_stamp_rows::read(self.host.as_ref()).await?;
        let current = match &stored {
            StoredStampRead::Unprovisioned => None,
            StoredStampRead::Stamped(stamp) => Some(CurrentStamp::of_stored(stamp)),
        };
        let decision = decide_organization_statement_replacement(current.as_ref(), &verified)
            .map_err(|reason| match reason {
                OrganizationStatementReplacementRefusal::OrganizationMismatch => {
                    Failure::OrganizationMismatch
                }
                OrganizationStatementReplacementRefusal::StaleOrganizationStatement => {
                    Failure::StaleStatement
                }
            })?;
        let (stamp, kind, request_id) = match decision {
            OrganizationStatementReplacement::AlreadyProvisioned => {
                if !stored_trusted || !stored_unexpired(&stored, now) {
                    return Err(Failure::StaleStatement);
                }
                let repair = async {
                    self.validated_timers().await?;
                    crate::central_mail_store::applied(
                        self.host
                            .run_batch(StatementBatch {
                                statements: vec![],
                                alarm: crate::central_mail_store::alarm_update(),
                            })
                            .await?,
                    )
                }
                .await;
                repair.map_err(|error| match error {
                    StoreFailure::Unavailable | StoreFailure::AnswerLost => Failure::OutcomeUnknown,
                    StoreFailure::Corrupt { .. } => Failure::Unavailable,
                })?;
                self.signin_authority.forget();
                return Ok(ProvisionOrganizationResult::Provisioned {});
            }
            OrganizationStatementReplacement::Initialize => (
                organization_stamp_rows::insert(
                    &self.organization,
                    request.organization_statement.as_str(),
                    now.unix_millis(),
                    verified.issued_at().0,
                    verified.expires_at().0,
                )?,
                OrganizationChangeKind::OrganizationProvisioned,
                self.organization.as_str().to_owned(),
            ),
            OrganizationStatementReplacement::Replace => {
                let StoredStampRead::Stamped(previous) = &stored else {
                    return Err(Failure::Unavailable);
                };
                (
                    organization_stamp_rows::replace(
                        previous,
                        request.organization_statement.as_str(),
                        now.unix_millis(),
                        verified.issued_at().0,
                        verified.expires_at().0,
                    )?,
                    OrganizationChangeKind::StampRenewed,
                    verified.digest().as_str().to_owned(),
                )
            }
        };
        let record = OrganizationChangeRecord {
            id: uuid::Uuid::parse_str(&self.ids.new_id().to_string())
                .map_err(|_| Failure::Unavailable)?,
            request_id: request_id.try_into()?,
            kind,
            origin: OrganizationChangeOrigin::SignInProvisioning {},
            target_id: self.organization.as_str().to_owned(),
            created_at_millis: now.unix_millis(),
        };
        self.validated_timers().await?;
        let statements = vec![stamp, organization_change_rows::insert(&record)?];
        #[cfg(feature = "test-support")]
        self.pauses
            .reached_central(crate::PausePoint::AfterSignInSample)
            .await;
        match self
            .host
            .run_batch(StatementBatch {
                statements,
                alarm: crate::central_mail_store::alarm_update(),
            })
            .await?
        {
            BatchOutcome::Applied { .. } => {}
            _ => return Err(Failure::StaleStatement),
        }
        self.signin_authority.forget();
        #[cfg(feature = "test-support")]
        if self.take_lost_provisioning_reply() {
            return Err(Failure::OutcomeUnknown);
        }
        Ok(ProvisionOrganizationResult::Provisioned {})
    }
    pub async fn read_statement_digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        use OrganizationProvisioningFailure as Failure;
        if request.organization_id != self.organization {
            return Err(Failure::OrganizationMismatch);
        }
        let _turn = self.take_central_turn().await;
        self.prepare_signin_authority().await?;
        let now = self.clock.now();
        match self.signin_authority.standing(now) {
            StampStanding::Unprovisioned => return Err(Failure::Unprovisioned),
            StampStanding::Refused => return Err(Failure::Unavailable),
            StampStanding::Valid => {}
        }
        let StoredStampRead::Stamped(stamp) =
            organization_stamp_rows::read(self.host.as_ref()).await?
        else {
            return Err(Failure::Unprovisioned);
        };
        let check = self.signin_authority.check().ok_or(Failure::Unavailable)?;
        let verified = check
            .verify_statement(stamp.statement())
            .await
            .map_err(|_| Failure::Unavailable)?;
        Ok(OrganizationStatementDigestReply {
            digest: verified.digest().as_str().to_owned(),
            expires_at: verified.expires_at().0,
            signing_key_id: verified.key_id().to_owned(),
        })
    }
    pub async fn deliver_authorization_revision(
        &self,
        revision: &AuthorizationRevision,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        use OrganizationProvisioningFailure as Failure;
        if revision.organization_id() != &self.organization {
            return Err(Failure::OrganizationMismatch);
        }
        let _turn = self.take_central_turn().await;
        self.prepare_signin_authority().await?;
        let now = self.now()?;
        if self
            .signin_authority
            .standing(SyncTime::from_unix_millis(now.timestamp_millis()))
            != StampStanding::Valid
        {
            return Err(Failure::Unavailable);
        }
        let plan = crate::authorization_revision_plans::prepare_authorization_revision(
            self.host.as_ref(),
            revision,
            now,
        )
        .await?;
        self.apply_revision_plan(plan).await
    }
}
fn stored_unexpired(stored: &StoredStampRead, now: SyncTime) -> bool {
    let StoredStampRead::Stamped(stamp) = stored else {
        return false;
    };
    crate::organization_authority::stored_statement_unexpired(stamp.statement(), now)
}
impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    async fn apply_revision_plan(
        &self,
        plan: crate::authorization_revision_plans::AuthorizationRevisionPlan,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        use crate::person_request::floor_delivery::FloorDelivery;
        use OrganizationProvisioningFailure as Failure;
        let revision = match plan.delivery {
            FloorDelivery::Acknowledged(revision) => revision,
            FloorDelivery::Conflict => return Err(Failure::StaleStatement),
            FloorDelivery::OrganizationFloorMissing => {
                return Err(Failure::OrganizationFloorMissing);
            }
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
            BatchOutcome::Applied { .. } => Ok(AuthorizationRevisionAcknowledgement {
                acknowledged_revision: revision,
            }),
            _ => Err(Failure::Unavailable),
        }
    }
}
