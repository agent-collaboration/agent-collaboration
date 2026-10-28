//! Central accepts the directory, native registration completes, then the first write initializes it.
use super::{
    ProjectAdmission, ProjectAdmissionError,
    request_gate::{ReadyRequest, RequestGate, RequestGateInput},
};
use chrono::Utc;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::request_canonical::creation_hash;
use message_board::{
    Board, CanonicalHash, ChangeOutcome, Description, Fact, FactId, FactKind, FleetFeedEntry,
    FleetPosition, LifecycleFact, ProjectCreateRequest, ProjectDirectoryEntry, ProjectId,
    ProjectRejection, ProjectState, Record, RecordEnvelope, RecordPlacement, RecordScope,
    RecordTime, RecordedActor, Refusal, RequestDecision, RequestLedgerEntry, RequestResult,
    ScopeRefusal, StoredDecision, SubjectRevision, WriteReply,
};

// The caller waits briefly; the durable Ready recovery owns eventual completion.
const PROJECT_CREATION_READY_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

impl ProjectAdmission {
    pub async fn create_project(
        &self,
        request: &ProjectCreateRequest,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        match self.create_project_reply(request).await {
            Ok(reply) => Ok(reply),
            Err(error) => Ok(WriteReply {
                result: super::write_gate::refusal_result(error)?,
                reader: None,
            }),
        }
    }

    async fn create_project_reply(
        &self,
        request: &ProjectCreateRequest,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let hash = creation_hash(request)?;
        let Some(Record::Board(board)) =
            self.fleet.record(&request.board_id.clone().into()).await?
        else {
            return Ok(WriteReply {
                result: RequestResult::ScopeUnavailable(ScopeRefusal::BoardNotFound {
                    board: request.board_id.clone(),
                }),
                reader: None,
            });
        };
        let directory = FleetRequest {
            request_id: request.request_id.clone(),
            workspace: board.workspace_id.clone(),
            actor: RecordedActor::declared(request.actor.clone()),
            acting_for: request.acting_for.clone(),
            protocol_version: request.protocol_version,
            operation: FleetOperation::ProjectDirectoryCreate {
                project: request.project_id.clone(),
                board: request.board_id.clone(),
                title: request.title.clone(),
            },
        };
        let directory_reply = self
            .fleet
            .submit_creation(request, &directory, &self.record_origin())
            .await?;
        match directory_reply.result {
            RequestResult::Decided(RequestDecision::Accepted { .. }) => {}
            RequestResult::Decided(RequestDecision::Rejected { reasons }) => {
                let [refusal] = reasons.as_slice() else {
                    return Err(ProjectAdmissionError::InvalidRecord(
                        "directory creation refusal must have one typed reason".to_owned(),
                    ));
                };
                return Ok(WriteReply {
                    result: RequestResult::ScopeUnavailable(ScopeRefusal::ProjectRejected {
                        project: request.project_id.clone(),
                        reason: ProjectRejection::Directory {
                            refusal: refusal.clone(),
                        },
                    }),
                    reader: None,
                });
            }
            result => {
                return Ok(WriteReply {
                    result,
                    reader: None,
                });
            }
        }
        let completion = self.complete_accepted_creation(request, &board, hash).await;
        match completion {
            Ok(reply) => Ok(reply),
            Err(error) => {
                tracing::warn!(%error, project = ?request.project_id, "directory accepted; local project completion remains pending");
                Ok(pending_creation_reply(&request.project_id))
            }
        }
    }

    async fn complete_accepted_creation(
        &self,
        request: &ProjectCreateRequest,
        board: &Board,
        hash: CanonicalHash,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let input = RequestGateInput {
            project: &request.project_id,
            request: &request.request_id,
            hash: &hash,
        };
        if let Some(result) = self.rejected_request(&input).await? {
            return Ok(WriteReply {
                result,
                reader: None,
            });
        }
        if !self
            .creation_registration_is_ready(&request.project_id)
            .await?
        {
            match tokio::time::timeout(
                PROJECT_CREATION_READY_WAIT,
                self.fleet
                    .wait_for_project_registration(&request.project_id),
            )
            .await
            {
                Ok(Ok(_)) | Err(_) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, project = ?request.project_id, "accepted project registration is pending")
                }
            }
        }
        // Only the Host registration port may bootstrap the writer after Welcome.
        let Some(store) = self.registry.existing_project(&request.project_id).await? else {
            return Ok(pending_creation_reply(&request.project_id));
        };
        if !matches!(
            store.registration_outcome().await?,
            fleet_sync_writer::WriterRegistrationOutcome::Ready { .. }
        ) {
            return Ok(pending_creation_reply(&request.project_id));
        }
        let ready = self.begin_request(input, self.record_origin()).await?;
        let result = match ready {
            RequestGate::Reply(result) => result,
            RequestGate::Ready(ready) => {
                let ReadyRequest {
                    mut write,
                    epoch,
                    high_water,
                } = *ready;
                let now = Utc::now();
                let actor = RecordedActor::declared(request.actor.clone());
                let scope = RecordScope {
                    workspace: board.workspace_id.clone(),
                    board: Some(request.board_id.clone()),
                    project: Some(request.project_id.clone()),
                };
                let placement = write.record_placement(high_water);
                let envelope = |id| RecordEnvelope {
                    id,
                    scope: scope.clone(),
                    actor: actor.clone(),
                    request: Some(request.request_id.clone()),
                    position: placement.clone(),
                    time: RecordTime::Router(now),
                };
                let decision = if write
                    .record(&request.project_id.clone().into())
                    .await?
                    .is_some()
                {
                    StoredDecision::Rejected {
                        reasons: vec![Refusal::AlreadyExists(request.project_id.clone().into())],
                    }
                } else {
                    let state = Record::Project(ProjectState {
                        envelope: envelope(request.project_id.clone().into()),
                        project_id: request.project_id.clone(),
                        description: request
                            .description
                            .clone()
                            .map_or_else(|| Description::try_from(String::new()), Ok)
                            .map_err(|error| {
                                ProjectAdmissionError::InvalidRecord(error.to_string())
                            })?,
                        revision: SubjectRevision::INITIAL,
                        closed_at: None,
                    });
                    let entry = ProjectDirectoryEntry {
                        envelope: RecordEnvelope {
                            id: request.project_id.clone().into(),
                            scope: RecordScope {
                                project: None,
                                ..scope.clone()
                            },
                            actor: actor.clone(),
                            request: Some(request.request_id.clone()),
                            position: RecordPlacement::Fleet(FleetFeedEntry {
                                position: FleetPosition::ZERO,
                                origin: self.record_origin(),
                            }),
                            time: RecordTime::Router(now),
                        },
                        id: request.project_id.clone(),
                        board: request.board_id.clone(),
                        title: request.title.clone(),
                    };
                    let fact_id = FactId::generate();
                    let created = Record::Fact(Fact {
                        envelope: envelope(fact_id.clone().into()),
                        fact_id,
                        kind: FactKind::Lifecycle(LifecycleFact::ProjectCreated {
                            entry: Box::new(entry),
                            creating_request: request.request_id.clone(),
                            acting_for: request.acting_for.clone(),
                        }),
                        links: vec![request.project_id.clone().into()],
                        producer_time: now,
                        anchor: None,
                        late: false,
                    });
                    let references = vec![state.reference(), created.reference()];
                    let state_cursor = write.append_record(state).await?;
                    let created_cursor = write.append_record(created).await?;
                    let positions = vec![
                        write.record_placement(state_cursor),
                        write.record_placement(created_cursor),
                    ];
                    StoredDecision::accepted(
                        references.into_iter().zip(positions).collect(),
                        ChangeOutcome::Changed,
                    )
                };
                let ledger = RequestLedgerEntry {
                    envelope: envelope(request.request_id.clone().into()),
                    request_id: request.request_id.clone(),
                    canonical_hash: hash,
                    accepted_in: Some(epoch),
                    decision: decision.clone(),
                };
                let reply = write.request_decision(&ledger).await?;
                write.append_record(Record::Request(ledger)).await?;
                write.commit().await?;
                RequestResult::Decided(reply)
            }
        };
        let reader = self
            .project_reader_state(&request.project_id, &request.actor)
            .await?;
        Ok(WriteReply { result, reader })
    }

    async fn creation_registration_is_ready(
        &self,
        project: &ProjectId,
    ) -> Result<bool, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(false);
        };
        Ok(matches!(
            store.registration_outcome().await?,
            fleet_sync_writer::WriterRegistrationOutcome::Ready { .. }
        ))
    }

    /// Startup and Welcome retry accepted inputs; the creating ledger owns completion.
    pub async fn recover_creating_projects(&self) -> Result<(), ProjectAdmissionError> {
        self.resume_creation_inputs(None).await
    }

    /// The Host calls this through its existing weak admission reference on native Ready.
    pub async fn resume_creating_project(
        &self,
        project: &ProjectId,
    ) -> Result<(), ProjectAdmissionError> {
        self.resume_creation_inputs(Some(project)).await
    }

    async fn resume_creation_inputs(
        &self,
        project: Option<&ProjectId>,
    ) -> Result<(), ProjectAdmissionError> {
        let mut after = String::new();
        loop {
            let page = self.fleet.accepted_creation_page(&after, 100).await?;
            for request in &page {
                if project.is_none_or(|project| project == &request.project_id)
                    && let Err(error) = self.create_project(request).await
                {
                    tracing::warn!(project = ?request.project_id, %error, "project creation recovery unavailable for this project");
                }
            }
            let Some(last) = page.last() else { break };
            after = last.request_id.as_str().to_owned();
            if page.len() < 100 {
                break;
            }
        }
        Ok(())
    }
}

fn pending_creation_reply(project: &ProjectId) -> WriteReply {
    WriteReply {
        result: RequestResult::ProjectCreationPending {
            project: project.clone(),
        },
        reader: None,
    }
}
