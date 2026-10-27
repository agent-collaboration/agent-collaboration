//! Declared membership writes use project admission; delivery reads join local windows.
use super::{BoardOperations, presence_for_reader, subscription_view, subscription_wait_result};
use collaboration_protocol::{
    ThreadSubscribeRequest, ThreadSubscriptionWaitFilter, ThreadSubscriptionWaitRequest,
    ThreadSubscriptionWaitResult, ThreadSubscriptionsRequest, ThreadSubscriptionsResult,
    ThreadUnsubscribeRequest,
};
use message_board::public_replies::WriteReply;
use message_board::{
    BoardError, BoardOperation, ProjectId, ProjectRequest, SubscriptionTarget, ThreadRenewRequest,
};
impl BoardOperations<'_> {
    pub(super) async fn subscription_project(
        &self,
        scope: &SubscriptionTarget,
    ) -> Result<ProjectId, BoardError> {
        match scope {
            SubscriptionTarget::Project { project_id } => Ok(project_id.clone()),
            SubscriptionTarget::Thread { thread } => self.thread_project(thread).await,
        }
    }
    pub async fn thread_renew(
        &self,
        request: ThreadRenewRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.subscription_project(&request.scope).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            protocol_version: request.protocol_version,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            operation: BoardOperation::ThreadRenew {
                scope: request.scope.stored_scope(),
            },
        })
        .await
    }
    pub async fn thread_subscribe(
        &self,
        request: ThreadSubscribeRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.subscription_project(&request.scope).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            protocol_version: request.protocol_version,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            operation: BoardOperation::ThreadSubscribe {
                scope: request.scope.stored_scope(),
                policy: request.policy,
            },
        })
        .await
    }
    pub async fn thread_unsubscribe(
        &self,
        request: ThreadUnsubscribeRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.subscription_project(&request.scope).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            protocol_version: request.protocol_version,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            operation: BoardOperation::ThreadUnsubscribe {
                scope: request.scope.stored_scope(),
            },
        })
        .await
    }
    pub async fn thread_subscriptions(
        &self,
        request: ThreadSubscriptionsRequest,
    ) -> Result<ThreadSubscriptionsResult, BoardError> {
        // The owner returns the snapshot from the same pass that reconciled the reader.
        let records = if let Some(delivery) = &self.identity.subscription_delivery {
            delivery.reader_subscriptions(request.actor.clone()).await?
        } else {
            let interactions = self
                .identity
                .interaction_store
                .as_ref()
                .ok_or_else(BoardError::board_unavailable)?;
            let windows = crate::SubscriptionWindowStore::new(
                self.admission()?.registry.clone(),
                interactions.clone(),
            );
            windows
                .list_reader_subscriptions(&request.actor, self.identity.subscription_clock.now())
                .await?
        };
        let presence = match &self.identity.subscription_presence {
            Some(probe) => presence_for_reader(&request.actor, probe).await,
            None => collaboration_protocol::ThreadSubscriptionPresence::Unreachable {
                reason: "presence unavailable".to_owned(),
            },
        };
        Ok(ThreadSubscriptionsResult {
            subscriptions: records
                .records
                .into_iter()
                .map(|record| subscription_view(record, presence.clone()))
                .collect(),
            unavailable: records.unavailable,
        })
    }
    pub async fn thread_wait(
        &self,
        request: ThreadSubscriptionWaitRequest,
        maximum_root_notice_bytes: usize,
    ) -> Result<ThreadSubscriptionWaitResult, BoardError> {
        request
            .validate()
            .map_err(|error| BoardError::invalid_field("request", error.to_string()))?;
        let mut renewals = Vec::new();
        for entry in &request.renewals {
            let result = async {
                let project_id = self.subscription_project(&entry.scope).await?;
                let reply = self
                    .project_write(ProjectRequest {
                        request_id: entry.request_id.clone(),
                        protocol_version: request.protocol_version,
                        project_id,
                        actor: request.actor.clone(),
                        acting_for: request.acting_for.clone(),
                        operation: BoardOperation::ThreadRenew {
                            scope: entry.scope.stored_scope(),
                        },
                    })
                    .await?;
                Ok::<_, BoardError>(reply)
            }
            .await;
            let reply = match result {
                Ok(reply) => reply,
                Err(reason) => {
                    return Ok(ThreadSubscriptionWaitResult {
                        renewals,
                        outcome: collaboration_protocol::ThreadSubscriptionWaitOutcome::Refused {
                            reason,
                        },
                    });
                }
            };
            renewals.push(collaboration_protocol::SubscriptionRenewalOutcome {
                request_id: entry.request_id.clone(),
                scope: entry.scope.clone(),
                reply,
            });
        }
        let renewal_bytes = serde_json::to_vec(&renewals)
            .map_err(|_| BoardError::board_unavailable())?
            .len();
        let Some(maximum_root_notice_bytes) =
            maximum_root_notice_bytes.checked_sub(renewal_bytes.saturating_sub(2))
        else {
            return Ok(ThreadSubscriptionWaitResult {
                renewals,
                outcome: collaboration_protocol::ThreadSubscriptionWaitOutcome::Refused {
                    reason: BoardError::invalid_field(
                        "renewals",
                        "renewal results leave no room for a wait batch",
                    ),
                },
            });
        };
        let filter = match request.filter {
            ThreadSubscriptionWaitFilter::All {} => crate::SubscriptionWaitFilter::All,
            ThreadSubscriptionWaitFilter::Roots { threads } => {
                crate::SubscriptionWaitFilter::Roots(threads)
            }
            ThreadSubscriptionWaitFilter::Project { project_id } => {
                crate::SubscriptionWaitFilter::Project(project_id)
            }
        };
        let outcome = match &self.identity.subscription_delivery {
            Some(delivery) => match delivery
                .wait(
                    request.actor,
                    filter,
                    request.max_wait_seconds,
                    maximum_root_notice_bytes,
                )
                .await
            {
                Ok(result) => subscription_wait_result(result),
                Err(reason) => {
                    collaboration_protocol::ThreadSubscriptionWaitOutcome::Refused { reason }
                }
            },
            None => collaboration_protocol::ThreadSubscriptionWaitOutcome::Refused {
                reason: BoardError::board_unavailable(),
            },
        };
        Ok(ThreadSubscriptionWaitResult { renewals, outcome })
    }
}
