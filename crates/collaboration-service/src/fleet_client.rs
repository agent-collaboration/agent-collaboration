//! Fleet writes and authoritative reads share the Host's one machine session.
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload, BoardReadPage,
    BoardReadScope, FleetRequestEnvelope,
};
use fleet_admission::{FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use fleet_sync_tokio_runner::SessionRequests;
use message_board::{Record, RecordRef, RequestDecision, RequestId, RequestResult};
use message_board_storage::{
    FleetCache, FleetOutbox, FleetOutboxEntry, FleetOutboxState, FleetStorageError,
    RouterLocalStorageError,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

#[path = "fleet_client/cache_refresh.rs"]
mod cache_refresh;
mod synchronization;
use synchronization::SynchronizationRequests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetReply {
    pub result: RequestResult,
}

#[derive(Debug, thiserror::Error)]
pub enum FleetClientError {
    #[error(transparent)]
    Synchronization(Arc<FleetClientError>),
    #[error("Fleet synchronization generation exhausted")]
    SynchronizationGenerationExhausted,
    #[error("fleet directory is unavailable: {0}")]
    Directory(#[from] std::io::Error),
    #[error(transparent)]
    Authoritative(#[from] FleetStorageError),
    #[error(transparent)]
    Local(#[from] RouterLocalStorageError),
    #[error(transparent)]
    Project(#[from] crate::ProjectRegistryError),
    #[error(transparent)]
    Registration(#[from] crate::ProjectRegistrationError),
    #[error("the machine session is unavailable")]
    Unavailable,
    #[error("central returned an invalid fleet reply: {field}")]
    InvalidReply { field: &'static str },
    #[error(transparent)]
    Codec(#[from] serde_json::Error),
    #[error("the replacement snapshot expired below {floor:?}")]
    SnapshotExpired { floor: message_board::FleetPosition },
}

pub struct FleetClient {
    session: Option<SessionRequests<BoardConsumerProtocol>>,
    outbox: FleetOutbox,
    cache: FleetCache,
    admission_mutex: Mutex<()>,
    synchronization: SynchronizationRequests,
    registration: Arc<dyn crate::ProjectRegistration>,
}

impl FleetClient {
    /// Forward transient post bodies without entering the directory outbox.
    pub(crate) async fn forward_post(
        &self,
        project: message_board::ProjectId,
        post: board_sync_protocol::ForwardedPost,
    ) -> Result<board_sync_protocol::ForwardOutcome, FleetClientError> {
        let session = self.session.as_ref().ok_or(FleetClientError::Unavailable)?;
        forwarded_answer(
            session
                .request(OperationRequest::Consumer(
                    BoardConsumerRequest::ForwardedPost { project, post },
                ))
                .await,
        )
    }

    /// Central binds the report's installation and time to this machine session.
    pub async fn report_archive_seen(
        &self,
        report: fleet_admission::ArchiveReport,
    ) -> Result<fleet_admission::ArchiveReportOutcome, FleetClientError> {
        let answer = self
            .consumer_request(BoardConsumerRequest::ArchiveSeen(
                board_sync_protocol::ArchiveSeen {
                    board: report.board,
                    project: report.project,
                    position: report.position,
                },
            ))
            .await?;
        let BoardConsumerAnswer::ArchiveSeen(payload) = answer else {
            return Err(FleetClientError::InvalidReply {
                field: "archiveSeen",
            });
        };
        Ok(payload.decode::<fleet_admission::ArchiveReportOutcome>()?)
    }

    pub async fn open(
        directory: &Path,
        registration: Arc<dyn crate::ProjectRegistration>,
        session: Option<SessionRequests<BoardConsumerProtocol>>,
    ) -> Result<Self, FleetClientError> {
        tokio::fs::create_dir_all(directory).await?;
        let outbox = FleetOutbox::open(&directory.join("fleet-outbox.sqlite")).await?;
        let cache = FleetCache::open(&directory.join("fleet-cache.sqlite")).await?;
        Ok(Self {
            session,
            outbox,
            cache,
            admission_mutex: Mutex::new(()),
            synchronization: SynchronizationRequests::default(),
            registration,
        })
    }

    /// Settle the outbox before an Accepted central answer starts registration.
    pub async fn settle_central_reply(
        &self,
        request: &RequestId,
        decision: &RequestDecision,
    ) -> Result<Option<crate::ProjectRegistrationState>, FleetClientError> {
        let queued = self
            .outbox
            .request(request)
            .await?
            .ok_or(RouterLocalStorageError::InvalidRecord)?;
        self.outbox.mark_decided(request, decision).await?;
        self.start_accepted_directory_registration(&queued.request, decision)
            .await
    }

    pub async fn recover_project_registrations(&self) -> Result<(), FleetClientError> {
        let mut after = String::new();
        loop {
            let page = self.outbox.settled_directory_page(&after, 100).await?;
            for queued in &page {
                let FleetOutboxState::Decided { decision } = &queued.state else {
                    return Err(RouterLocalStorageError::InvalidRecord.into());
                };
                self.start_accepted_directory_registration(&queued.request, decision)
                    .await?;
            }
            let Some(last) = page.last() else { break };
            after = last.request.request_id.as_str().to_owned();
            if page.len() < 100 {
                break;
            }
        }
        Ok(())
    }

    pub(crate) async fn wait_for_project_registration(
        &self,
        project: &message_board::ProjectId,
    ) -> Result<crate::ProjectRegistrationState, FleetClientError> {
        Ok(self.registration.wait_ready(project).await?)
    }

    async fn start_accepted_directory_registration(
        &self,
        request: &FleetRequest,
        decision: &RequestDecision,
    ) -> Result<Option<crate::ProjectRegistrationState>, FleetClientError> {
        if !matches!(decision, RequestDecision::Accepted { .. }) {
            return Ok(None);
        }
        match &request.operation {
            fleet_admission::FleetOperation::ProjectDirectoryCreate { project, .. } => {
                Ok(Some(self.registration.start(project).await?))
            }
            _ => Ok(None),
        }
    }

    pub async fn submit(
        &self,
        request: &FleetRequest,
        _origin: &message_board::ActorOrigin,
    ) -> Result<FleetReply, FleetClientError> {
        let guard = self.admission_mutex.lock().await;
        let result = self.submit_admitted(request).await;
        self.finish_submission_sync().await;
        drop(guard);
        result
    }

    async fn submit_admitted(
        &self,
        request: &FleetRequest,
    ) -> Result<FleetReply, FleetClientError> {
        let queued = match self.outbox.enqueue(request).await {
            Ok(queued) => queued,
            Err(RouterLocalStorageError::ConflictingRequest) => {
                return Ok(FleetReply {
                    result: RequestResult::ConflictingRequest,
                });
            }
            Err(error) => return Err(error.into()),
        };
        if let FleetOutboxState::Decided { decision } = &queued.state {
            return Ok(FleetReply {
                result: RequestResult::Decided(decision.clone()),
            });
        }
        let reply = self.send_queued(&queued).await?;
        if matches!(reply.result, RequestResult::PendingUnreachable) {
            return Ok(reply);
        }
        self.refresh_cache().await?;
        Ok(reply)
    }

    /// Queue the complete creating input before any native project file is opened.
    pub async fn submit_creation(
        &self,
        input: &message_board::ProjectCreateRequest,
        directory: &FleetRequest,
        _origin: &message_board::ActorOrigin,
    ) -> Result<FleetReply, FleetClientError> {
        let guard = self.admission_mutex.lock().await;
        let result = self.submit_creation_admitted(input, directory).await;
        self.finish_submission_sync().await;
        drop(guard);
        result
    }

    async fn submit_creation_admitted(
        &self,
        input: &message_board::ProjectCreateRequest,
        directory: &FleetRequest,
    ) -> Result<FleetReply, FleetClientError> {
        let queued = match self.outbox.enqueue_creation(directory, input).await {
            Ok(queued) => queued,
            Err(RouterLocalStorageError::ConflictingRequest) => {
                return Ok(FleetReply {
                    result: RequestResult::ConflictingRequest,
                });
            }
            Err(error) => return Err(error.into()),
        };
        if let FleetOutboxState::Decided { decision } = &queued.state {
            // A keyed creating replay remains local after its decision is durable.
            return Ok(FleetReply {
                result: RequestResult::Decided(decision.clone()),
            });
        }
        let reply = self.send_queued(&queued).await?;
        if matches!(reply.result, RequestResult::PendingUnreachable) {
            return Ok(reply);
        }
        self.refresh_cache().await?;
        Ok(reply)
    }

    async fn finish_submission_sync(&self) {
        // A deferred sync must not replace the submitted request's own result.
        if let Err(error) = self.synchronize_pending_admitted().await {
            tracing::warn!(%error, "Fleet synchronization after submission failed");
        }
    }

    pub async fn creation_input(
        &self,
        request: &RequestId,
    ) -> Result<Option<message_board::ProjectCreateRequest>, FleetClientError> {
        Ok(self.outbox.creation_input(request).await?)
    }

    pub async fn accepted_creation_page(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<message_board::ProjectCreateRequest>, FleetClientError> {
        Ok(self.outbox.accepted_creation_page(after, limit).await?)
    }

    async fn synchronize_admitted(&self) -> Result<(), FleetClientError> {
        // Durable Accepted decisions can resume without a remote metadata read.
        self.recover_project_registrations().await?;
        for queued in self.outbox.pending().await? {
            // A newly decided answer starts registration inside send_queued.
            self.send_queued(&queued).await?;
        }
        self.refresh_cache().await
    }

    async fn send_queued(&self, queued: &FleetOutboxEntry) -> Result<FleetReply, FleetClientError> {
        if let FleetOutboxState::Decided { decision } = &queued.state {
            return Ok(FleetReply {
                result: RequestResult::Decided(decision.clone()),
            });
        }
        let body = BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: queued.request.request_id.clone(),
            operation: BoardPayload::encode(&queued.request)?,
        });
        let answer = match self.consumer_request(body).await {
            Ok(answer) => answer,
            Err(FleetClientError::Unavailable) => {
                return Ok(FleetReply {
                    result: RequestResult::PendingUnreachable,
                });
            }
            Err(error) => return Err(error),
        };
        let BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        } = answer
        else {
            return Err(FleetClientError::InvalidReply { field: "fleet" });
        };
        if request_id != queued.request.request_id {
            return Err(FleetClientError::InvalidReply { field: "requestId" });
        }
        let result = match decision.decode::<FleetRequestResult>()? {
            FleetRequestResult::Decided(decision) => {
                self.settle_central_reply(&request_id, &decision).await?;
                RequestResult::Decided(decision)
            }
            FleetRequestResult::ConflictingRequest => RequestResult::ConflictingRequest,
        };
        Ok(FleetReply { result })
    }

    async fn consumer_request(
        &self,
        request: BoardConsumerRequest,
    ) -> Result<BoardConsumerAnswer, FleetClientError> {
        let session = self.session.as_ref().ok_or(FleetClientError::Unavailable)?;
        match session
            .request(OperationRequest::Consumer(request))
            .await
            .map_err(|_| FleetClientError::Unavailable)?
        {
            OperationAnswer::Consumer(answer) => Ok(answer),
            _ => Err(FleetClientError::InvalidReply { field: "operation" }),
        }
    }

    /// Receipts belong to the authoritative session read, never the local record cache.
    pub async fn request_effects(
        &self,
        request: &message_board::RequestEffectsRequest,
    ) -> Result<message_board::RequestEffectsRead, FleetClientError> {
        let message_board::RequestEffectsScope::Fleet { workspace_id } = &request.scope else {
            return Err(FleetClientError::InvalidReply { field: "scope" });
        };
        match self
            .consumer_request(BoardConsumerRequest::Read {
                scope: BoardReadScope::RequestEffects {
                    workspace: workspace_id.clone(),
                    request: request.request.clone(),
                    after: request.after.clone(),
                },
                cursor: None,
            })
            .await?
        {
            BoardConsumerAnswer::Read(BoardReadPage::RequestEffects { result, .. }) => Ok(result),
            _ => Err(FleetClientError::InvalidReply {
                field: "requestEffects",
            }),
        }
    }

    pub async fn record(&self, reference: &RecordRef) -> Result<Option<Record>, FleetClientError> {
        Ok(self.cache.record(reference).await?)
    }

    pub async fn records(&self) -> Result<Vec<Record>, FleetClientError> {
        Ok(self.cache.snapshot().await?.records)
    }

    pub async fn read_snapshot(
        &self,
    ) -> Result<message_board_storage::FleetCacheSnapshot, FleetClientError> {
        Ok(self.cache.read_snapshot().await?)
    }

    pub async fn workspace_projects(
        &self,
        workspace: &message_board::WorkspaceId,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut projects: std::collections::BTreeSet<_> = self
            .cache
            .workspace_project_ids(workspace)
            .await?
            .into_iter()
            .collect();
        projects.extend(self.outbox.pending_project_ids(workspace).await?);
        Self::without_rejected(projects, registry).await
    }

    pub async fn scoped_projects(
        &self,
        board: Option<&message_board::BoardId>,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut projects: std::collections::BTreeSet<_> = match board {
            Some(board) => self.cache.board_project_ids(board).await?,
            None => self.cache.project_ids().await?,
        }
        .into_iter()
        .collect();
        projects.extend(self.outbox.pending_scoped_projects(board).await?);
        Self::without_rejected(projects, registry).await
    }
    pub async fn pending_workspace_projects(
        &self,
        workspace: &message_board::WorkspaceId,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        Self::without_rejected(
            self.outbox
                .pending_project_ids(workspace)
                .await?
                .into_iter()
                .collect(),
            registry,
        )
        .await
    }
    async fn without_rejected(
        projects: std::collections::BTreeSet<message_board::ProjectId>,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut selected = Vec::new();
        for project in projects {
            if !registry.is_rejected_project(&project).await? {
                selected.push(project);
            }
        }
        Ok(selected)
    }
    pub async fn pending_request(
        &self,
        request: &message_board::RequestId,
    ) -> Result<Option<FleetOutboxEntry>, FleetClientError> {
        Ok(self.outbox.pending_request(request).await?)
    }
    pub async fn workspace_page_keys(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<message_board::WorkspaceId>, FleetClientError> {
        Ok(self.cache.workspace_page_keys(after, limit).await?)
    }
    pub async fn pending_requests(&self) -> Result<Vec<FleetOutboxEntry>, FleetClientError> {
        Ok(self.outbox.pending().await?)
    }
    pub async fn close(self) {
        self.cache.close().await;
        self.outbox.close().await;
    }
}

fn forwarded_answer(
    answer: Result<OperationAnswer<BoardConsumerProtocol>, durable_steps::CallFailure>,
) -> Result<board_sync_protocol::ForwardOutcome, FleetClientError> {
    use board_sync_protocol::ForwardOutcome;
    use durable_steps::{CallFailure, NotSentReason};
    match answer {
        Ok(OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome))) => Ok(outcome),
        Ok(_) => Err(FleetClientError::InvalidReply {
            field: "forwardedPost",
        }),
        // A timeout cannot prove the holder did not commit. The caller's stable
        // request ID is the only retry identity; the sender stores no decision.
        Err(CallFailure::AnswerLost) => Ok(ForwardOutcome::Unanswered),
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        }) => Ok(ForwardOutcome::Overloaded),
        Err(CallFailure::NotSent { .. } | CallFailure::Retryable { .. }) => {
            Err(FleetClientError::Unavailable)
        }
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use board_sync_protocol::ForwardOutcome;
    use durable_steps::{CallFailure, CallRetryReason, NotSentReason};

    #[test]
    #[allow(clippy::panic_in_result_fn)] // Assertions compare outcomes; Result propagates fixture encoding failures.
    fn forwarded_answer_preserves_central_outcomes() -> Result<(), Box<dyn std::error::Error>> {
        let outcomes = [
            ForwardOutcome::SenderHolds,
            ForwardOutcome::NotHeld,
            ForwardOutcome::HolderUnreachable {
                last_seen: Some(serde_json::from_value(serde_json::json!(1234))?),
            },
            ForwardOutcome::Unanswered,
            ForwardOutcome::Overloaded,
            ForwardOutcome::Answered {
                reply: BoardPayload::encode(&serde_json::json!({"opaque":"synthetic answer"}))?,
            },
        ];
        for outcome in outcomes {
            assert_eq!(
                forwarded_answer(Ok(OperationAnswer::Consumer(
                    BoardConsumerAnswer::ForwardedPost(outcome.clone())
                )))?,
                outcome
            );
        }
        Ok(())
    }

    #[test]
    fn forwarded_answer_keeps_unknown_dispatch_distinct_from_no_effect() {
        assert!(matches!(
            forwarded_answer(Err(CallFailure::AnswerLost)),
            Ok(ForwardOutcome::Unanswered)
        ));
        assert!(matches!(
            forwarded_answer(Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded
            })),
            Ok(ForwardOutcome::Overloaded)
        ));
        assert!(matches!(
            forwarded_answer(Err(CallFailure::NotSent {
                reason: NotSentReason::Unreachable { last_seen: None }
            })),
            Err(FleetClientError::Unavailable)
        ));
        assert!(matches!(
            forwarded_answer(Err(CallFailure::Retryable {
                reason: CallRetryReason {
                    code: "no-new-effect".into(),
                    detail: "synthetic transport refusal".into()
                }
            })),
            Err(FleetClientError::Unavailable)
        ));
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)] // The assertion checks refusal; Result propagates fixture encoding failures.
    fn forwarded_answer_refuses_another_consumer_family() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(matches!(
            forwarded_answer(Ok(OperationAnswer::Consumer(
                BoardConsumerAnswer::ArchiveSeen(BoardPayload::encode(&())?)
            ))),
            Err(FleetClientError::InvalidReply {
                field: "forwardedPost"
            })
        ));
        Ok(())
    }
}
