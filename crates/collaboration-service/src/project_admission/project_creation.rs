//! File-first project creation and durable requeue into the fleet directory.
use super::{ProjectAdmission, ProjectAdmissionError, granted_epoch, request_gate::*};
use chrono::Utc;
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;
use sha2::{Digest, Sha256};

impl ProjectAdmission {
    pub async fn create_project(
        &self,
        request: &ProjectCreateRequest,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let hash = creation_hash(request)?;
        let input = RequestGateInput {
            project: &request.project_id,
            request: &request.request_id,
            observed_epoch: None,
            hash: &hash,
        };
        if let Some(result) = self.rejected_request(&input).await? {
            return Ok(WriteReply {
                result,
                reader: None,
            });
        }
        if let Err(result) = granted_epoch(self.leases.grant(&request.project_id).await?) {
            return Ok(WriteReply {
                result,
                reader: None,
            });
        }
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
        self.registry.open_project(&request.project_id).await?;
        let ready = self.begin_request(input).await?;
        let result = match ready {
            RequestGate::Reply(result) => result,
            RequestGate::Ready(ready) => {
                let ReadyRequest {
                    mut write,
                    epoch,
                    high_water,
                } = *ready;
                let now = Utc::now();
                let actor = RecordedActor {
                    actor: ActorKind::Declared(request.actor.clone()),
                    installation: self.installation.clone(),
                    machine: self.machine.clone(),
                };
                let scope = RecordScope {
                    workspace: board.workspace_id.clone(),
                    board: Some(request.board_id.clone()),
                    project: Some(request.project_id.clone()),
                };
                let envelope = |id| RecordEnvelope {
                    id,
                    scope: scope.clone(),
                    actor: actor.clone(),
                    request: Some(request.request_id.clone()),
                    position: RecordPosition::Project(high_water),
                    router_time: now,
                };
                let decision = if write
                    .record(&request.project_id.clone().into())
                    .await?
                    .is_some()
                {
                    RequestDecision::Rejected {
                        reasons: vec![Refusal::AlreadyExists(request.project_id.clone().into())],
                    }
                } else {
                    write
                        .record_writer_epoch(&WriterEpoch {
                            project: request.project_id.clone(),
                            number: epoch,
                            holder: self.installation.clone(),
                            start_position: FeedPosition::ZERO,
                            end: None,
                        })
                        .await?;
                    let state = Record::Project(ProjectState {
                        envelope: envelope(request.project_id.clone().into()),
                        project_id: request.project_id.clone(),
                        status: None,
                        pinned_definition: None,
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
                            position: RecordPosition::Fleet(FleetPosition::ZERO),
                            router_time: now,
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
                    let positions = vec![
                        RecordPosition::Project(write.append_record(state).await?),
                        RecordPosition::Project(write.append_record(created).await?),
                    ];
                    RequestDecision::Accepted {
                        records: references,
                        positions,
                        change: ChangeOutcome::Changed,
                    }
                };
                write
                    .append_record(Record::Request(RequestLedgerEntry {
                        envelope: envelope(request.request_id.clone().into()),
                        request_id: request.request_id.clone(),
                        canonical_hash: hash,
                        observed_epoch: None,
                        accepted_in: Some(epoch),
                        decision: decision.clone(),
                        resubmits: request.resubmits.clone(),
                    }))
                    .await?;
                write.commit().await?;
                RequestResult::Decided(decision)
            }
        };
        let result = if matches!(
            result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ) {
            self.publish_created_project(&request.project_id, &request.request_id, result)
                .await?
        } else {
            result
        };
        let reader = self
            .project_reader_state(&request.project_id, &request.actor)
            .await?;
        Ok(WriteReply { result, reader })
    }

    async fn publish_created_project(
        &self,
        project: &ProjectId,
        request: &RequestId,
        accepted: RequestResult,
    ) -> Result<RequestResult, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(accepted);
        };
        let mut read = store.snapshot().await?;
        let ledger = read.record(&request.clone().into()).await?;
        let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "creating project state is missing".into(),
            ));
        };
        let creation_key = read.creation_fact_key(request).await?.ok_or_else(|| {
            ProjectAdmissionError::InvalidRecord("creating fact is missing".into())
        })?;
        let creation = read.record(&creation_key).await?;
        read.finish().await?;
        let resubmits = match ledger {
            Some(Record::Request(ledger)) => ledger.resubmits,
            _ => {
                return Err(ProjectAdmissionError::InvalidRecord(
                    "creating ledger is missing".into(),
                ));
            }
        };
        let Some(Record::Fact(creation)) = creation else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "creating fact is missing".into(),
            ));
        };
        if creation.envelope.scope != state.envelope.scope {
            return Err(message_board_storage::ProjectStorageError::InvalidRecord {
                record: creation_key,
                field: "scope",
            }
            .into());
        }
        if !matches!(&creation.kind, FactKind::Lifecycle(LifecycleFact::ProjectCreated { creating_request, .. }) if creating_request == request)
        {
            return Err(ProjectAdmissionError::InvalidRecord(
                "creating fact has wrong request".into(),
            ));
        }
        let FactKind::Lifecycle(LifecycleFact::ProjectCreated {
            entry,
            creating_request,
            acting_for,
        }) = creation.kind
        else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "creating fact has wrong kind".into(),
            ));
        };
        let result = self
            .fleet
            .submit(
                &FleetRequest {
                    request_id: creating_request,
                    workspace: entry.envelope.scope.workspace,
                    actor: creation.envelope.actor.clone(),
                    acting_for,
                    resubmits,
                    protocol_version: ProtocolVersion::CURRENT,
                    operation: FleetOperation::ProjectDirectoryCreate {
                        project: entry.id,
                        board: entry.board,
                        title: entry.title,
                    },
                },
                &FleetIds {
                    generated_record: RecordId::generate(),
                },
            )
            .await?;
        if let RequestResult::Decided(RequestDecision::Rejected { reasons }) = result.result {
            let Some(reason) = reasons.into_iter().next() else {
                return Err(ProjectAdmissionError::InvalidRecord(
                    "directory rejected without a reason".into(),
                ));
            };
            let mut write = store.begin_admission().await?;
            write
                .void_accepted_requests(
                    VoidCause::ProjectRejected {
                        reason: Box::new(reason),
                    },
                    creation.envelope.actor,
                    Utc::now(),
                )
                .await?;
            let commit = write.commit_retaining_writer().await?;
            self.registry.orphan_project(project, commit).await?;
            let reader = self
                .registry
                .rejected_project(project)
                .await?
                .ok_or_else(|| {
                    ProjectAdmissionError::InvalidRecord("orphan reader is missing".into())
                })?;
            let ledger = reader.ledger(request).await?.ok_or_else(|| {
                ProjectAdmissionError::InvalidRecord("voided creating ledger is missing".into())
            })?;
            return Ok(RequestResult::Decided(ledger.decision));
        }
        Ok(accepted)
    }

    pub async fn recover_creating_projects(&self) -> Result<(), ProjectAdmissionError> {
        for project in self.registry.project_ids().await? {
            if let Err(error) = self.recover_creating_project(&project).await {
                tracing::warn!(?project, %error, "project creation recovery unavailable for this project");
            }
        }
        Ok(())
    }

    async fn recover_creating_project(
        &self,
        project: &ProjectId,
    ) -> Result<(), ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(());
        };
        let mut read = store.snapshot().await?;
        if let Some(ledger) = read.project_rejection_request().await? {
            let RequestDecision::Voided { cause } = ledger.decision else {
                return Err(ProjectAdmissionError::InvalidRecord(
                    "rejected creation decision is invalid".into(),
                ));
            };
            read.finish().await?;
            let mut write = store.begin_admission().await?;
            write
                .void_accepted_requests(cause, ledger.envelope.actor, Utc::now())
                .await?;
            let commit = write.commit_retaining_writer().await?;
            self.registry.orphan_project(project, commit).await?;
            return Ok(());
        }
        let mut after = String::new();
        let mut pending = Vec::new();
        loop {
            let mut keys = read.creation_fact_page_keys(&after, 101).await?;
            let more = keys.len() > 100;
            keys.truncate(100);
            if let Some(last) = keys.last() {
                after = last.id.as_str().to_owned();
            }
            let mut creating_requests = Vec::new();
            for stored in read.record_batch(&keys).await? {
                let Record::Fact(Fact {
                    kind:
                        FactKind::Lifecycle(LifecycleFact::ProjectCreated {
                            creating_request, ..
                        }),
                    ..
                }) = stored.decode()?
                else {
                    return Err(ProjectAdmissionError::InvalidRecord(
                        "creating fact has wrong kind".into(),
                    ));
                };
                creating_requests.push(creating_request);
            }
            let request_keys: Vec<_> = creating_requests
                .iter()
                .cloned()
                .map(RecordRef::from)
                .collect();
            let mut decisions = std::collections::BTreeMap::new();
            for stored in read.record_batch(&request_keys).await? {
                let Record::Request(ledger) = stored.decode()? else {
                    return Err(ProjectAdmissionError::InvalidRecord(
                        "creating ledger has wrong kind".into(),
                    ));
                };
                decisions.insert(ledger.request_id, ledger.decision);
            }
            for request in creating_requests {
                if let Some(decision @ RequestDecision::Accepted { .. }) = decisions.get(&request) {
                    pending.push((request, decision.clone()));
                }
            }
            if !more {
                break;
            }
        }
        read.finish().await?;
        for (request, decision) in pending {
            self.publish_created_project(project, &request, RequestResult::Decided(decision))
                .await?;
        }
        Ok(())
    }
}

fn creation_hash(request: &ProjectCreateRequest) -> Result<CanonicalHash, ProjectAdmissionError> {
    let bytes = serde_json::to_vec(&(
        &request.project_id,
        &request.board_id,
        &request.title,
        &request.actor,
        &request.acting_for,
        &request.resubmits,
        request.protocol_version,
    ))
    .map_err(request_canonical::CanonicalRequestError::Encoding)?;
    CanonicalHash::try_from(format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))
}
