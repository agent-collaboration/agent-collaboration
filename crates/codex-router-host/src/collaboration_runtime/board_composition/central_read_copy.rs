//! Snapshot reads borrow the machine session and never create project writers.
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload, BoardReadPage,
    BoardReadScope,
};
use collaboration_service::{
    CentralReadError, ProjectReadCopyFuture, ProjectReadCopySource, PulledProjectReadCopy,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, Engine, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::{
    CopyStatus, CopyStatusAnswer, DatabaseId, InstallAttemptId, OperationAnswer, OperationRequest,
    ReceiverId, ReplicationAnswer, ReplicationRequest,
};
use fleet_sync_tokio_runner::{
    CallEnvelope, CallRouter, InstallCoordinator, InstallFile, InstallFileRequest,
    InstallFileResult, InstallRefusal, InstallRequest, InstallSnapshot, InstallSource,
    InstallStatus, ObjectKey, ObjectOptions, ObjectStore, SessionRequests, StagingError,
    StagingStore, TokioHost, TokioObject,
};
use message_board::ProjectId;
use runtime_primitives::SystemClock;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::watch;

type ReadHost = TokioHost<ReadCopyCalls>;
#[cfg(all(test, feature = "runtime-test-support"))]
#[path = "tests/read_copy_page_gate.rs"]
mod proof_gate;
struct InstallParts {
    coordinator: Arc<InstallCoordinator<ReadHost>>,
    target: Arc<TokioObject<ReadHost>>,
}
struct ReadCopyCalls {
    requests: SessionRequests<BoardConsumerProtocol>,
    receiver: ReceiverId,
    database: DatabaseId,
    parts: OnceLock<std::sync::Weak<InstallParts>>,
    #[cfg(all(test, feature = "runtime-test-support"))]
    page_gate: Option<Arc<proof_gate::StagedPageGate>>,
}

pub(super) struct CentralReadCopies {
    requests: SessionRequests<BoardConsumerProtocol>,
    receiver: ReceiverId,
    welcomed: watch::Receiver<bool>,
    root: PathBuf,
    #[cfg(all(test, feature = "runtime-test-support"))]
    page_gate: Option<Arc<proof_gate::StagedPageGate>>,
}
impl CentralReadCopies {
    pub fn new(
        root: &std::path::Path,
        requests: SessionRequests<BoardConsumerProtocol>,
        receiver: ReceiverId,
        welcomed: watch::Receiver<bool>,
    ) -> Self {
        Self {
            requests,
            receiver,
            welcomed,
            root: root
                .join("board-read-cache")
                .join(durable_steps::Uuid::now_v7().to_string()),
            #[cfg(all(test, feature = "runtime-test-support"))]
            page_gate: None,
        }
    }
    #[cfg(all(test, feature = "runtime-test-support"))]
    pub(super) fn pause_after_staged_page_for_test(mut self) -> (Self, proof_gate::ReadCopyPause) {
        let (gate, pause) = proof_gate::StagedPageGate::new();
        self.page_gate = Some(Arc::new(gate));
        (self, pause)
    }
    async fn pull_generation(
        &self,
        project: &ProjectId,
    ) -> Result<PulledProjectReadCopy, CentralReadError> {
        if !*self.welcomed.borrow() || self.requests.is_closed() {
            return Err(CentralReadError::Unavailable);
        }
        let query = BoardPayload::encode(&fleet_admission::DirectoryQuery::default())
            .map_err(|_| CentralReadError::Corrupt)?;
        let answer = self
            .requests
            .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
                scope: BoardReadScope::Directory { query },
                cursor: None,
            }))
            .await
            .map_err(|_| CentralReadError::Unavailable)?;
        let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
            entries,
            ..
        })) = answer
        else {
            return Err(CentralReadError::Corrupt);
        };
        let mut directory = None;
        for entry in entries {
            let entry: fleet_admission::DirectoryEntry =
                entry.decode().map_err(|_| CentralReadError::Corrupt)?;
            if entry.project == *project && directory.replace(entry).is_some() {
                return Err(CentralReadError::Corrupt);
            }
        }
        let lease = directory.ok_or(CentralReadError::Uninitialized)?.lease_side;
        let database = DatabaseId::new(
            durable_steps::Uuid::parse_str(project.as_str())
                .map_err(|_| CentralReadError::Corrupt)?,
        )
        .map_err(|_| CentralReadError::Corrupt)?;
        let root = self
            .root
            .join(project.as_str())
            .join(durable_steps::Uuid::now_v7().to_string());
        let calls = Arc::new(ReadCopyCalls {
            requests: self.requests.clone(),
            receiver: self.receiver,
            database,
            parts: OnceLock::new(),
            #[cfg(all(test, feature = "runtime-test-support"))]
            page_gate: self.page_gate.clone(),
        });
        let host = Arc::new(TokioHost::new(calls.clone(), SystemClock));
        let objects = ObjectStore::open(root.join("objects"))
            .await
            .map_err(|_| CentralReadError::Unavailable)?;
        let result = async {
            let control = objects
                .object(
                    ObjectKey::new("control").map_err(|_| CentralReadError::Corrupt)?,
                    ObjectOptions::default(),
                    host.clone(),
                )
                .await
                .map_err(|_| CentralReadError::Unavailable)?;
            let target = objects
                .object(
                    ObjectKey::new("copy").map_err(|_| CentralReadError::Corrupt)?,
                    ObjectOptions {
                        foreign_keys: false,
                    },
                    host.clone(),
                )
                .await
                .map_err(|_| CentralReadError::Unavailable)?;
            target
                .transaction(|scope| {
                    Box::pin(async move {
                        let mut migrator = sqlx::migrate::Migrator::with_migrations(
                            fleet_sync_receiver::RECEIVER_MIGRATOR
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>(),
                        );
                        migrator.dangerous_set_table_name("_fleet_sync_copy_migrations");
                        migrator
                            .run_direct(None, scope.executor(), false)
                            .await
                            .map_err(StoreError::backend)
                    })
                })
                .await
                .map_err(|_| CentralReadError::Unavailable)?;
            let staging = Arc::new(
                StagingStore::open(root.join("staging"))
                    .await
                    .map_err(|_| CentralReadError::Unavailable)?,
            );
            let coordinator = Arc::new(
                InstallCoordinator::open(control.clone(), staging)
                    .await
                    .map_err(|_| CentralReadError::Unavailable)?,
            );
            let mut engine = Engine::new(control, host.clone());
            engine
                .register(InstallSnapshot::new(host))
                .map_err(|_| CentralReadError::Corrupt)?;
            let parts = Arc::new(InstallParts {
                coordinator: coordinator.clone(),
                target: target.clone(),
            });
            calls
                .parts
                .set(Arc::downgrade(&parts))
                .map_err(|_| CentralReadError::Corrupt)?;
            let attempt = InstallAttemptId::new(durable_steps::Uuid::now_v7())
                .map_err(|_| CentralReadError::Corrupt)?;
            let handle = coordinator
                .start(
                    InstallRequest {
                        attempt,
                        target: target.key().clone(),
                        database,
                        source: self.receiver,
                    },
                    &target,
                )
                .await
                .map_err(|_| CentralReadError::Unavailable)?;
            // A pull has a bounded lifetime; it never runs a second connection or
            // leaves a reader pointing at the target while the installer writes it.
            let record = tokio::time::timeout(std::time::Duration::from_secs(30), async {
                engine
                    .drive_due()
                    .await
                    .map_err(|_| CentralReadError::Unavailable)?;
                let record = handle
                    .status()
                    .await
                    .map_err(|_| CentralReadError::Unavailable)?;
                if matches!(record.status, InstallStatus::Running { .. }) {
                    Err(CentralReadError::Unavailable)
                } else {
                    Ok(record)
                }
            })
            .await
            .map_err(|_| CentralReadError::Unavailable)??;
            let snapshot = match record.status {
                InstallStatus::Ready { snapshot } => snapshot,
                InstallStatus::Failed {
                    reason: durable_steps::OperationFailure::Refused { code, .. },
                    ..
                } if code == "source_uninitialized" => return Err(CentralReadError::Uninitialized),
                InstallStatus::Failed {
                    reason: durable_steps::OperationFailure::Refused { code, .. },
                    ..
                } if code == "source_retired" => return Err(CentralReadError::Retired),
                _ => return Err(CentralReadError::Corrupt),
            };
            let copy = coordinator
                .read_copy(snapshot.install_id, &target)
                .await
                .map_err(|_| CentralReadError::Corrupt)?;
            let copy = message_board_storage::ProjectReadCopy::from_verified(project.clone(), copy)
                .await
                .map_err(|error| match error {
                    message_board_storage::ProjectStorageError::SchemaNotReady(_) => {
                        CentralReadError::UnsupportedSchema
                    }
                    _ => CentralReadError::Corrupt,
                })?;
            Ok(PulledProjectReadCopy { copy, lease })
        }
        .await;
        let closed = objects.close().await;
        result.and_then(|copy| {
            closed
                .map(|()| copy)
                .map_err(|_| CentralReadError::Unavailable)
        })
    }
}
impl ProjectReadCopySource for CentralReadCopies {
    fn pull<'a>(&'a self, project: &'a ProjectId) -> ProjectReadCopyFuture<'a> {
        Box::pin(async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(60),
                self.pull_generation(project),
            )
            .await
            .map_err(|_| CentralReadError::Unavailable)?
        })
    }
}

fn not_sent() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, CallFailure> {
    serde_json::from_value(value).map_err(|_| CallFailure::AnswerLost)
}
fn encode(value: impl Serialize) -> Result<serde_json::Value, CallFailure> {
    serde_json::to_value(value).map_err(|_| CallFailure::AnswerLost)
}

impl ReadCopyCalls {
    async fn replicate(
        &self,
        request: ReplicationRequest<board_sync_protocol::BoardAnnotation>,
    ) -> Result<ReplicationAnswer, CallFailure> {
        match self
            .requests
            .request(OperationRequest::Replication(request))
            .await?
        {
            OperationAnswer::Replication(ReplicationAnswer::CopyUnavailable { reason }) => {
                Err(fleet_sync_receiver::retryable_copy(reason))
            }
            OperationAnswer::Replication(ReplicationAnswer::OutcomeUnknown { .. }) => {
                Err(CallFailure::AnswerLost)
            }
            OperationAnswer::Replication(answer) => Ok(answer),
            _ => Err(CallFailure::AnswerLost),
        }
    }
}
impl CallRouter for ReadCopyCalls {
    async fn dispatch(&self, call: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        match call.kind {
            InstallSource::KIND => {
                let source: InstallSource = decode(call.target)?;
                if source.database != self.database || source.receiver != self.receiver {
                    return Err(not_sent());
                }
                let answer: Result<CopyStatus, InstallRefusal> = match self
                    .replicate(ReplicationRequest::CopyStatus {
                        database: self.database,
                    })
                    .await?
                {
                    ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)) => Ok(status),
                    ReplicationAnswer::CopyStatus(CopyStatusAnswer::NotInitialized) => {
                        Err(InstallRefusal {
                            code: "source_uninitialized".into(),
                            detail: "central copy is uninitialized".into(),
                        })
                    }
                    ReplicationAnswer::CopyStatus(CopyStatusAnswer::Retired) => {
                        Err(InstallRefusal {
                            code: "source_retired".into(),
                            detail: "central copy is retired".into(),
                        })
                    }
                    _ => return Err(CallFailure::AnswerLost),
                };
                encode(answer)
            }
            InstallFile::KIND => {
                let file: InstallFile = decode(call.target)?;
                let request: InstallFileRequest = decode(call.request)?;
                let parts = self
                    .parts
                    .get()
                    .and_then(std::sync::Weak::upgrade)
                    .ok_or_else(not_sent)?;
                let record = parts
                    .coordinator
                    .record(file.install_id)
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                if record.request.database != self.database
                    || record.request.source != self.receiver
                    || record.request.target != *parts.target.key()
                {
                    return Err(not_sent());
                }
                let database = self.database;
                let result = match request {
                    InstallFileRequest::Prepare { source } => {
                        parts.coordinator.prepare(file.install_id, &source).await
                    }
                    InstallFileRequest::Page { page, cursor } => {
                        let fetching = cursor.clone();
                        let answer = parts
                            .coordinator
                            .fetch_page(file.install_id, page, cursor.as_ref(), || async move {
                                match self
                                    .replicate(ReplicationRequest::SnapshotPage {
                                        database,
                                        cursor: fetching,
                                    })
                                    .await
                                    .map_err(|failure| StagingError::SourceCall { failure })?
                                {
                                    ReplicationAnswer::SnapshotPage(answer) => Ok(answer),
                                    _ => Err(StagingError::SourceCall {
                                        failure: CallFailure::AnswerLost,
                                    }),
                                }
                            })
                            .await;
                        #[cfg(all(test, feature = "runtime-test-support"))]
                        if let Some(gate) = &self.page_gate {
                            match &answer {
                                Ok(InstallFileResult::Page(
                                    fleet_sync_tokio_runner::StagedPage::Receipt(receipt),
                                )) => {
                                    gate.after_page(receipt.clone(), parts.target.path().to_owned())
                                        .await?
                                }
                                Ok(InstallFileResult::Page(
                                    fleet_sync_tokio_runner::StagedPage::Moved { position },
                                )) => gate.moved(*position),
                                _ => {}
                            }
                        }
                        answer
                    }
                    InstallFileRequest::Verify => {
                        let position = record
                            .status
                            .snapshot()
                            .ok_or(CallFailure::AnswerLost)?
                            .position;
                        parts
                            .coordinator
                            .verify_from(file.install_id, || async move {
                                match self
                                    .replicate(ReplicationRequest::SnapshotManifest {
                                        database,
                                        position,
                                    })
                                    .await
                                    .map_err(|failure| StagingError::SourceCall { failure })?
                                {
                                    ReplicationAnswer::SnapshotManifest(answer) => Ok(answer),
                                    _ => Err(StagingError::SourceCall {
                                        failure: CallFailure::AnswerLost,
                                    }),
                                }
                            })
                            .await
                    }
                    InstallFileRequest::Publish => parts
                        .coordinator
                        .publish(file.install_id, &parts.target)
                        .await
                        .map(InstallFileResult::Outcome),
                    InstallFileRequest::Ready => parts
                        .coordinator
                        .mark_read_copy_ready(file.install_id, &parts.target)
                        .await
                        .map(InstallFileResult::Outcome),
                };
                match result {
                    Ok(answer) => encode(Ok::<_, InstallRefusal>(answer)),
                    Err(StagingError::SourceCall { failure }) => Err(failure),
                    // Match the SDK writer installer: transport/storage failures
                    // leave the last completed generation eligible for stale reads.
                    Err(
                        StagingError::Storage(_)
                        | StagingError::Io { .. }
                        | StagingError::Rules(fleet_sync_receiver::SnapshotInstallError::Storage(_))
                        | StagingError::Publish(
                            fleet_sync_tokio_runner::PublishError::Storage(_)
                            | fleet_sync_tokio_runner::PublishError::Io { .. }
                            | fleet_sync_tokio_runner::PublishError::CheckpointBusy { .. },
                        ),
                    ) => Err(CallFailure::AnswerLost),
                    Err(error) => encode(Err::<InstallFileResult, _>(InstallRefusal {
                        code: "read_copy_invalid".into(),
                        detail: error.to_string(),
                    })),
                }
            }
            _ => Err(not_sent()),
        }
    }
}
