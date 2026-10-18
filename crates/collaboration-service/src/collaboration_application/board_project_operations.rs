//! Project operations retain their declared input and pass once into admission.
use super::{BoardOperations, admission_error};
use message_board::*;

impl BoardOperations<'_> {
    pub(super) async fn thread_project(
        &self,
        reference: &ThreadRef,
    ) -> Result<ProjectId, BoardError> {
        self.admission()?.registry.thread_project(reference).await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(|| {
                let mut error = super::missing_record(&reference.thread.clone().into());
                error.message = "Thread not found in open local projects; a thread held elsewhere needs its project.".to_owned();
                error
            })
    }
    pub async fn project_write(&self, request: ProjectRequest) -> Result<WriteReply, BoardError> {
        let reply = self
            .admission()?
            .submit(&request)
            .await
            .map_err(admission_error)?;
        if let Some(delivery) = &self.identity.subscription_delivery {
            if let Err(error) = delivery
                .queue_project_intents(self.admission()?.clone(), request.project_id.clone())
                .await
            {
                tracing::warn!(%error,"board intent consumer could not be queued after admission");
            }
            // Reconciliation is a post-commit event; delivery owns its local progress.
            if let Err(error) = delivery
                .reconcile_project(request.actor, request.project_id)
                .await
            {
                tracing::warn!(%error,"board delivery reconciliation failed after admission");
            }
        }
        Ok(reply)
    }
    pub async fn thread_create(
        &self,
        request: ThreadCreateRequest,
    ) -> Result<WriteReply, BoardError> {
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id: request.project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadCreate {
                post_id: request.post_id,
                about: request.about,
                text: request.text,
                references: post_references(request.references),
                participation: request.role,
                watch: request.watch,
                addressees: request.addressees,
            },
        })
        .await
    }
    pub async fn thread_refile(
        &self,
        request: ThreadRefileRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadRefile {
                thread_id: request.thread.thread,
                about: request.about,
            },
        })
        .await
    }
    pub async fn thread_post(&self, request: ThreadPostRequest) -> Result<WriteReply, BoardError> {
        let (project_id, operation) = match request.placement {
            PostTarget::Thread { thread } => (
                self.thread_project(&thread).await?,
                BoardOperation::ThreadPost {
                    post_id: request.post_id,
                    thread_id: thread.thread,
                    text: request.text,
                    references: post_references(request.references),
                    addressees: request.addressees,
                },
            ),
        };
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation,
        })
        .await
    }
    pub async fn thread_resolve(
        &self,
        request: ThreadResolveRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadResolve {
                thread_id: request.thread.thread,
                reason: request.reason,
            },
        })
        .await
    }
    pub async fn thread_unresolve(
        &self,
        request: ThreadUnresolveRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadUnresolve {
                thread_id: request.thread.thread,
            },
        })
        .await
    }
    pub async fn thread_join(&self, request: ThreadJoinRequest) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: None,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadJoin {
                thread_id: request.thread.thread,
                role: request.role,
                watch: request.watch,
                mode: request.mode,
                when_idle: request.when_idle,
                replace: request.replace,
                note: request.note,
            },
        })
        .await
    }
    pub async fn thread_leave(
        &self,
        request: ThreadLeaveRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: None,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadLeave {
                thread_id: request.thread.thread,
                after_leave: request.after_leave,
            },
        })
        .await
    }
    pub async fn thread_watch(
        &self,
        request: ThreadWatchRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadWatch {
                thread_id: request.thread.thread,
            },
        })
        .await
    }
    pub async fn thread_unwatch(
        &self,
        request: ThreadUnwatchRequest,
    ) -> Result<WriteReply, BoardError> {
        let project_id = self.thread_project(&request.thread).await?;
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: BoardOperation::ThreadUnwatch {
                thread_id: request.thread.thread,
            },
        })
        .await
    }
    pub async fn project_watch(
        &self,
        request: ProjectWatchRequest,
    ) -> Result<WriteReply, BoardError> {
        self.project_watch_choice(request, true).await
    }
    pub async fn project_unwatch(
        &self,
        request: ProjectWatchRequest,
    ) -> Result<WriteReply, BoardError> {
        self.project_watch_choice(request, false).await
    }
    async fn project_watch_choice(
        &self,
        request: ProjectWatchRequest,
        active: bool,
    ) -> Result<WriteReply, BoardError> {
        self.project_write(ProjectRequest {
            request_id: request.request_id,
            project_id: request.project_id,
            actor: request.actor,
            acting_for: request.acting_for,
            observed_epoch: request.observed_epoch,
            resubmits: request.resubmits,
            protocol_version: request.protocol_version,
            operation: if active {
                BoardOperation::ProjectWatch {}
            } else {
                BoardOperation::ProjectUnwatch {}
            },
        })
        .await
    }
    pub async fn inbox_acknowledge(
        &self,
        request: InboxAcknowledgeRequest,
    ) -> Result<InboxAcknowledgeReply, BoardError> {
        let mut outcomes = Vec::with_capacity(request.entries().len());
        for entry in request.entries() {
            let project = match &entry.scope {
                ReadScopeRef::Project { project_id } => Ok(project_id.clone()),
                ReadScopeRef::Thread { thread } => self.thread_project(thread).await,
            };
            let outcome = match project {
                Err(error) => unrouted_write_outcome(error, &entry.scope.stored_scope()),
                Ok(project_id) => match self
                    .project_write(ProjectRequest {
                        request_id: entry.request_id.clone(),
                        project_id,
                        actor: request.actor.clone(),
                        acting_for: request.acting_for.clone(),
                        observed_epoch: entry.observed_epoch,
                        resubmits: entry.resubmits.clone(),
                        protocol_version: request.protocol_version,
                        operation: BoardOperation::ReadMarkAdvance {
                            reader: request.actor.clone(),
                            scope: entry.scope.stored_scope(),
                            through: entry.through,
                        },
                    })
                    .await
                {
                    Ok(reply) => ProjectWriteCallResult::Replied { reply },
                    Err(reason) => ProjectWriteCallResult::Unanswered { reason },
                },
            };
            outcomes.push(AcknowledgementOutcome {
                request_id: entry.request_id.clone(),
                outcome,
            });
        }
        Ok(InboxAcknowledgeReply { outcomes })
    }
}
// This gate is only available before routing resolves a project. Once routed,
// admission's own reply/ledger owns every definitive decision.
fn unrouted_write_outcome(error: BoardError, scope: &ReadScope) -> ProjectWriteCallResult {
    let refusal = if matches!(
        error.kind,
        BoardFailureKind::InvalidRootPost | BoardFailureKind::ResourceNotFound
    ) {
        Some(Refusal::NotFound(match scope {
            ReadScope::Project { project_id } => project_id.clone().into(),
            ReadScope::Thread { thread_id } => thread_id.clone().into(),
        }))
    } else if error.stage == BoardFailureStage::Validation {
        Some(Refusal::InvalidShape {
            field: match &error.details {
                BoardErrorDetails::FieldConstraint { field, .. } => field.clone(),
                _ => "entry".to_owned(),
            },
        })
    } else {
        None
    };
    match refusal {
        Some(reason) => ProjectWriteCallResult::Replied {
            reply: WriteReply {
                result: RequestResult::ScopeUnavailable(ScopeRefusal::Refused {
                    reason: Box::new(reason),
                }),
                reader: None,
            },
        },
        None => ProjectWriteCallResult::Unanswered { reason: error },
    }
}
fn post_references(references: PostReferences) -> Vec<RecordRef> {
    Vec::from(references)
        .into_iter()
        .map(|reference| match reference {
            ReferenceTarget::Post { post_id } => post_id.into(),
            ReferenceTarget::Thread { thread_id } => thread_id.into(),
        })
        .collect()
}
