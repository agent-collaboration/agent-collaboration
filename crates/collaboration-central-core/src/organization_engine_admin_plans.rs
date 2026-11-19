//! P8's four admin operations on the existing organization-admin owner: the
//! reads answer from storage; retry and discard retain their answer under the
//! operation ID, so a replay returns it as CreateWorkspace's does.
use crate::{
    AdminRefusal, ObjectHost, OrganizationAdminResult, WorkspaceRuntime, WorkspaceStoreCalls,
    engine::admin::{AttemptRefusal, DiscardOutcome},
    organization_admin_contract::{
        EngineKindHealth, EngineRetiredItem, RetainedOrganizationAdminAnswer,
    },
    person_request::{AdminOperationId, EngineAttemptRequest, VerifiedPersonRequest},
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use central_storage::{AlarmUpdate, GuardedStatement, StoreFailure};
use workspace_model::{ActorIdentityId, Identity, PersonId, RequestId, Timestamp};

/// What the admin batch carries for one engine operation.
pub(crate) struct EngineAdminPlan {
    pub result: OrganizationAdminResult,
    pub statements: Vec<GuardedStatement>,
    pub alarm: AlarmUpdate,
    pub retained: Option<RetainedOrganizationAdminAnswer>,
    /// Discard already committed its batch through `handle`.
    pub committed: bool,
}
impl EngineAdminPlan {
    fn read(result: OrganizationAdminResult) -> Self {
        Self {
            result,
            statements: vec![],
            alarm: AlarmUpdate::Unchanged,
            retained: None,
            committed: false,
        }
    }
}

enum OperationReceipt {
    Replay(EngineAdminPlan),
    Fresh {
        operation: RequestId,
        actor: ActorIdentityId,
        statements: Vec<GuardedStatement>,
        cutoff: String,
    },
}

fn refusal(refusal: AttemptRefusal) -> OrganizationAdminResult {
    OrganizationAdminResult::Refused {
        reason: match refusal {
            AttemptRefusal::NotFound => AdminRefusal::NotFound,
            AttemptRefusal::NotRetired => AdminRefusal::NotRetired,
            AttemptRefusal::DiscardUnsupported => AdminRefusal::DiscardUnsupported,
            AttemptRefusal::Stale => AdminRefusal::Stale,
            AttemptRefusal::SubjectUnreadable => AdminRefusal::SubjectUnreadable,
        },
    }
}

fn retained_engine_result(encoded: &str) -> Result<OrganizationAdminResult, StoreFailure> {
    let answer: OrganizationAdminResult =
        serde_json::from_value(json(encoded, "runtime_request_log.answer")?)
            .map_err(|_| corrupt("runtime_request_log.answer"))?;
    match answer {
        OrganizationAdminResult::EngineRetried {}
        | OrganizationAdminResult::EngineDiscarded {}
        | OrganizationAdminResult::Refused { .. } => Ok(answer),
        OrganizationAdminResult::Applied { .. }
        | OrganizationAdminResult::Found { .. }
        | OrganizationAdminResult::Records { .. }
        | OrganizationAdminResult::EngineHealth { .. }
        | OrganizationAdminResult::EngineRetired { .. } => {
            Err(corrupt("runtime_request_log.answer"))
        }
    }
}

impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    pub(crate) async fn engine_health_result(&self) -> Result<EngineAdminPlan, StoreFailure> {
        let health = crate::engine::port::Engine::new(self)
            .engine_health()
            .await?;
        Ok(EngineAdminPlan::read(
            OrganizationAdminResult::EngineHealth {
                kinds: health
                    .kinds
                    .into_iter()
                    .map(|kind| EngineKindHealth {
                        kind: kind.kind,
                        failing: kind.failing,
                        retired: kind.retired,
                        oldest_first_failed_at: kind.oldest_first_failed_at,
                        last_failure: kind.last_failure,
                    })
                    .collect(),
                alarm: health.alarm,
                alarm_missing: health.alarm_missing,
            },
        ))
    }

    pub(crate) async fn engine_retired_result(
        &self,
        after: Option<uuid::Uuid>,
        limit: u16,
    ) -> Result<EngineAdminPlan, StoreFailure> {
        let (items, next) = crate::engine::port::Engine::new(self)
            .engine_retired_page(after, limit)
            .await?;
        Ok(EngineAdminPlan::read(
            OrganizationAdminResult::EngineRetired {
                items: items
                    .into_iter()
                    .map(|item| EngineRetiredItem {
                        attempt_id: item.attempt_id,
                        kind: item.kind,
                        subject_key: item.subject_key,
                        failure_class: item.failure_class,
                        count: item.count,
                        retired_at: item.retired_at,
                        last_failure: item.last_failure,
                    })
                    .collect(),
                next,
            },
        ))
    }

    async fn engine_operation_receipt(
        &self,
        person: &VerifiedPersonRequest,
        operation_id: &AdminOperationId,
        now: Timestamp,
    ) -> Result<OperationReceipt, StoreFailure> {
        let operation: RequestId = operation_id
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| corrupt("operation_id"))?;
        let identity = Identity::Person {
            person_id: PersonId::try_from(person.person_id.clone())
                .map_err(|_| corrupt("person_id"))?,
        };
        let actor = self.prepare_identity(&identity, now).await?;
        let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
        let logged = self
            .host
            .read(crate::workspace_request_log::lookup(
                crate::workspace_request_log::RequestLogStore::Organization,
                &operation,
                &cutoff,
            ))
            .await?;
        if logged.rows.len() > 1 {
            return Err(corrupt("runtime_request_log.request_id"));
        }
        let Some(row) = logged.rows.first() else {
            return Ok(OperationReceipt::Fresh {
                operation,
                actor: actor.actor,
                statements: actor.statements,
                cutoff,
            });
        };
        let stored_actor: ActorIdentityId = string(&logged, row, "caller")?
            .parse()
            .map_err(|_| corrupt("runtime_request_log.caller"))?;
        if stored_actor != actor.actor
            || string(&logged, row, "content_digest")? != person.intent_digest
        {
            return Ok(OperationReceipt::Replay(EngineAdminPlan::read(
                OrganizationAdminResult::Refused {
                    reason: AdminRefusal::Conflict,
                },
            )));
        }
        let encoded = string(&logged, row, "answer")?;
        let mut plan = EngineAdminPlan::read(retained_engine_result(encoded)?);
        plan.retained = Some(RetainedOrganizationAdminAnswer::validated(
            encoded.to_owned(),
        ));
        Ok(OperationReceipt::Replay(plan))
    }

    /// Retry: the guarded delete and its retained answer ride in the admin batch,
    /// which re-arms the alarm so an idle object wakes for the subject.
    pub(crate) async fn prepare_engine_retry(
        &self,
        person: &VerifiedPersonRequest,
        request: &EngineAttemptRequest,
        now: Timestamp,
    ) -> Result<EngineAdminPlan, StoreFailure> {
        let (operation, actor, mut statements, cutoff) = match self
            .engine_operation_receipt(person, &request.operation_id, now)
            .await?
        {
            OperationReceipt::Replay(plan) => return Ok(plan),
            OperationReceipt::Fresh {
                operation,
                actor,
                statements,
                cutoff,
            } => (operation, actor, statements, cutoff),
        };
        let result = match crate::engine::port::Engine::new(self)
            .plan_engine_retry(request.attempt_id)
            .await?
        {
            Ok(delete) => {
                statements.push(delete);
                OrganizationAdminResult::EngineRetried {}
            }
            Err(reason) => refusal(reason),
        };
        statements.extend(crate::workspace_request_log::retain(
            crate::workspace_request_log::RequestLogStore::Organization,
            &operation,
            &actor,
            &person.intent_digest,
            &result,
            &now.to_rfc3339(),
            &cutoff,
        )?);
        Ok(EngineAdminPlan {
            alarm: match result {
                OrganizationAdminResult::EngineRetried {} => AlarmUpdate::SetEngineFromQuery {
                    query: central_engine_model::next_due_sql(
                        crate::engine::query_registry::ALARM_INPUTS,
                    ),
                },
                _ => AlarmUpdate::Unchanged,
            },
            result,
            statements,
            retained: None,
            committed: false,
        })
    }

    /// Discard: the admission writes, the retained answer and the guarded delete
    /// commit with the subject's Discard transition; a refusal falls back to the
    /// ordinary admin batch with the refusal retained.
    pub(crate) async fn prepare_engine_discard(
        &self,
        person: &VerifiedPersonRequest,
        request: &EngineAttemptRequest,
        admission: &mut Vec<GuardedStatement>,
        now: Timestamp,
    ) -> Result<EngineAdminPlan, StoreFailure> {
        let (operation, actor, mut statements, cutoff) = match self
            .engine_operation_receipt(person, &request.operation_id, now)
            .await?
        {
            OperationReceipt::Replay(plan) => return Ok(plan),
            OperationReceipt::Fresh {
                operation,
                actor,
                statements,
                cutoff,
            } => (operation, actor, statements, cutoff),
        };
        let retain = |result: &OrganizationAdminResult| {
            crate::workspace_request_log::retain(
                crate::workspace_request_log::RequestLogStore::Organization,
                &operation,
                &actor,
                &person.intent_digest,
                result,
                &now.to_rfc3339(),
                &cutoff,
            )
        };
        let mut committed = admission.clone();
        committed.extend(statements.clone());
        committed.extend(retain(&OrganizationAdminResult::EngineDiscarded {})?);
        match crate::engine::port::Engine::new(self)
            .engine_discard(request.attempt_id, committed)
            .await?
        {
            DiscardOutcome::Committed => {
                admission.clear();
                // The re-arm is this event's last step; a failed publication is
                // observed and the next event re-publishes.
                let _ = crate::engine::port::Engine::new(self)
                    .publish_engine_query(central_engine_model::next_due_sql(
                        crate::engine::query_registry::ALARM_INPUTS,
                    ))
                    .await;
                Ok(EngineAdminPlan {
                    result: OrganizationAdminResult::EngineDiscarded {},
                    statements: vec![],
                    alarm: AlarmUpdate::Unchanged,
                    retained: None,
                    committed: true,
                })
            }
            DiscardOutcome::Refused(reason) => {
                let result = refusal(reason);
                statements.extend(retain(&result)?);
                Ok(EngineAdminPlan {
                    result,
                    statements,
                    alarm: AlarmUpdate::Unchanged,
                    retained: None,
                    committed: false,
                })
            }
        }
    }
}
