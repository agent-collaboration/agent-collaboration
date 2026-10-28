//! The physical restart child shares its cold gates across both real producers.
use super::*;
use crate::collaboration_runtime::board_composition::BoardComposition;
use fleet_sync_protocol::NoticePosition;
use fleet_sync_session::SessionClientStore;

#[derive(Clone, Copy)]
enum InstallProducer {
    Claim,
    Notice,
}
enum SnapshotRestartPhase {
    BeforeReady {
        producer: InstallProducer,
    },
    Resume {
        producer: InstallProducer,
        grant: ChildGrant,
    },
}
impl SnapshotRestartPhase {
    fn parse(phase: &str, grant: Option<ChildGrant>) -> Result<Self, io::Error> {
        match phase {
            "before-ready" => Ok(Self::BeforeReady {
                producer: InstallProducer::Claim,
            }),
            "receive-before-ready" => Ok(Self::BeforeReady {
                producer: InstallProducer::Notice,
            }),
            "resume" | "receive-resume" => Ok(Self::Resume {
                producer: if phase == "resume" {
                    InstallProducer::Claim
                } else {
                    InstallProducer::Notice
                },
                grant: grant
                    .ok_or_else(|| io::Error::other("resume omitted the actual retained grant"))?,
            }),
            _ => Err(io::Error::other("unknown snapshot restart phase")),
        }
    }
}

pub(super) async fn run(inputs: ChildInputs, phase: &str, board: BoardComposition) -> TestResult {
    let phase = SnapshotRestartPhase::parse(phase, inputs.grant.clone())?;
    let project = &inputs.request.project_id;
    let projects = board
        .session
        .as_ref()
        .ok_or("snapshot session")?
        .projects
        .clone();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let welcomed = projects.welcomed.clone();
    match phase {
        SnapshotRestartPhase::BeforeReady { producer } => {
            let running = board.start().ok_or("snapshot runtime")?;
            wait_welcome(welcomed).await?;
            // Parent holds the exact real grant notice while this existing
            // native probe is armed. Correlated answers remain live.
            let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
            let request_started = SystemClock.now();
            let grant = timeout(BOUND, async {
                loop {
                    let answer = projects
                        .requests
                        .request(OperationRequest::Lease(LeaseRequest::Claim {
                            attempt: fleet_sync_protocol::RequestId::try_from(
                                inputs.request.request_id.as_str().to_owned(),
                            )?,
                            database: database_id,
                        }))
                        .await
                        .map_err(|error| io::Error::other(format!("fixture grant: {error:?}")))?;
                    match answer {
                        OperationAnswer::Lease(LeaseAnswer::Claim(
                            HolderChangeAnswer::Granted {
                                epoch,
                                expires_at,
                                start,
                            },
                        )) => {
                            return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(ChildGrant {
                                epoch,
                                expires_at,
                                start,
                            });
                        }
                        OperationAnswer::Lease(LeaseAnswer::Claim(
                            HolderChangeAnswer::Pending { .. },
                        )) => {}
                        _ => return Err("fixture did not obtain a real grant".into()),
                    }
                }
            })
            .await??;
            let database = registry
                .prepare_claim_seed(project, seed_grant(&inputs, &grant, request_started)?)
                .await?;
            let observer =
                open_install_record_observer(&projects, project, database.clone()).await?;
            database.hold_bootstrap_after_rename();
            let mut claimed = if matches!(producer, InstallProducer::Claim) {
                let claiming = projects.clone();
                let request = inputs.request.clone();
                Some(tokio::spawn(async move { claiming.claim(&request).await }))
            } else {
                None
            };
            emit("RECEIVE_RESTART_ARMED ", serde_json::json!({"grant":grant}))?;
            let mut instruction = String::new();
            BufReader::new(tokio::io::stdin())
                .read_line(&mut instruction)
                .await?;
            let position = NoticePosition::new(instruction.trim().parse()?);
            let installed = if let Some(claim) = claimed.as_mut() {
                timeout(BOUND, async {
                    tokio::select! {
                        installed = database.wait_bootstrap_published() => installed.map_err(Into::into),
                        answer = claim => Err::<_, Box<dyn std::error::Error + Send + Sync>>(format!("claim ended before its native hold: {answer:?}").into()),
                    }
                }).await??
            } else {
                timeout(BOUND, database.wait_bootstrap_published()).await??
            };
            if let Some(claim) = &claimed {
                assert!(!claim.is_finished(), "claim completed before Ready");
            }
            let store = board_fleet_sync::MachineSessionStore::open(&inputs.root).await?;
            timeout(BOUND, async {
                loop {
                    if store
                        .load()
                        .await?
                        .is_some_and(|state| state.notice_cursor >= position)
                    {
                        return Ok::<_, fleet_sync_session::StoreError>(());
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await??;
            let record = observer.install_record(installed.install_id).await?;
            assert!(matches!(
                record.status,
                fleet_sync_tokio_runner::InstallStatus::Running { .. }
            ));
            assert!(matches!(
                registry.existing_project(project).await,
                Err(collaboration_runtime::ProjectRegistryError::Installing { .. })
            ));
            emit(
                "CLAIM_RESTART_PUBLISHED_NOT_READY ",
                serde_json::json!({"start":installed.position,"stage":"afterRenameBeforeReady","record":record,"acknowledged":true,"noticePosition":position}),
            )?;
            // These real owners remain alive until the parent's SIGKILL.
            let _running = running;
            let _claimed = claimed;
            std::future::pending::<TestResult>().await
        }
        SnapshotRestartPhase::Resume { producer, grant } => {
            // No pump/Welcome, raw Claim replay or preparation precedes these
            // classification checks. Neither producer can repair their subject.
            let unpublished = matches!(
                registry.existing_project(project).await,
                Err(collaboration_runtime::ProjectRegistryError::Installing { .. })
            );
            let open_refused = matches!(
                registry.open_project(project).await,
                Err(collaboration_runtime::ProjectRegistryError::Installing { .. })
            );
            emit(
                "CLAIM_RESTART_ADMISSION ",
                serde_json::json!({"unpublished":unpublished,"openRefused":open_refused,"receivingOnly":matches!(producer, InstallProducer::Notice)}),
            )?;
            assert!(unpublished && open_refused);
            let database = registry
                .prepare_claim_seed(project, seed_grant(&inputs, &grant, SystemClock.now())?)
                .await?;
            let receipt = database
                .installed_snapshot()
                .await?
                .ok_or("crash receipt")?;
            let write_refused = matches!(database.begin_write().await, Err(WriteRefusal::Seeding));
            assert!(write_refused && !receipt.ready);
            let observer =
                open_install_record_observer(&projects, project, database.clone()).await?;
            emit(
                "CLAIM_RESTART_REOPEN_GATE ",
                serde_json::json!({"unpublished":unpublished,"openRefused":open_refused,"writeRefused":write_refused,"ready":receipt.ready,"receivingOnly":matches!(producer, InstallProducer::Notice)}),
            )?;
            let mut instruction = String::new();
            BufReader::new(tokio::io::stdin())
                .read_line(&mut instruction)
                .await?;
            assert_eq!(instruction, "resume\n");
            let running = board.start().ok_or("resume runtime")?;
            wait_welcome(welcomed).await?;
            if matches!(producer, InstallProducer::Claim) {
                let answer = timeout(BOUND, async {
                    loop {
                        let answer = projects.claim(&inputs.request).await?;
                        if !matches!(answer, ProjectClaimReply::Pending { .. }) {
                            return Ok::<_, collaboration_contracts::ProjectClaimError>(answer);
                        }
                    }
                })
                .await??;
                assert_eq!(
                    answer,
                    ProjectClaimReply::Claimed {
                        epoch: grant.epoch,
                        expires_at: grant.expires_at,
                        start: grant.start
                    }
                );
            }
            let store = timeout(BOUND, async {
                loop {
                    if let Ok(Some(store)) = registry.existing_project(project).await
                        && matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch == grant.epoch)
                    { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(store); }
                    tokio::task::yield_now().await;
                }
            }).await??;
            let mut snapshot = store.snapshot().await?;
            assert_eq!(
                snapshot.record(&project.clone().into()).await?,
                Some(inputs.record)
            );
            snapshot.finish().await?;
            let write = store.begin_admission(origin()?).await?;
            assert_eq!(write.admitted_epoch()?.get(), 2);
            write.rollback().await?;
            let receipt = database
                .installed_snapshot()
                .await?
                .ok_or("resumed receipt")?;
            assert!(receipt.ready && receipt.verified);
            let record = observer.install_record(receipt.install.install_id).await?;
            assert!(matches!(
                record.status,
                fleet_sync_tokio_runner::InstallStatus::Ready { .. }
            ));
            emit(
                "CLAIM_RESTART_READY ",
                serde_json::json!({"record":record,"start":receipt.install.position,"epoch":grant.epoch.value(),"verified":receipt.verified,"ready":receipt.ready,"receivingOnly":matches!(producer, InstallProducer::Notice)}),
            )?;
            timeout(BOUND, projects.settle_operations()).await??;
            drop(observer);
            drop(store);
            drop(database);
            drop(registry);
            drop(projects);
            running.shutdown().await?;
            Arc::try_unwrap(admission)
                .map_err(|_| io::Error::other("snapshot admission still owned"))?
                .close()
                .await?;
            Ok(())
        }
    }
}

fn seed_grant(
    inputs: &ChildInputs,
    grant: &ChildGrant,
    request_started: fleet_sync_protocol::SyncTime,
) -> Result<BootstrapGrant, Box<dyn std::error::Error + Send + Sync>> {
    Ok(BootstrapGrant {
        lease: LeaseStatus {
            database: DatabaseId::try_from(inputs.request.project_id.as_str().to_owned())?,
            epoch: grant.epoch,
            holder: inputs.peer,
            state: LeaseState::Held,
            expires_at: grant.expires_at,
            remaining_ms: 0,
        },
        request_started,
        start: grant.start,
    })
}
pub(super) async fn open_install_record_observer(
    projects: &crate::collaboration_runtime::board_composition::project_writers::ProjectWriters,
    project: &ProjectId,
    database: Arc<fleet_sync_writer::ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>>,
) -> Result<
    NativeWriterRuntime<
        board_fleet_sync::BoardProjectSchema,
        board_sync_protocol::BoardConsumerProtocol,
    >,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let mut observer =
        NativeWriterRuntime::new(database, projects.receiver, projects.requests.clone()).await?;
    observer
        .enable_snapshot_installs(
            &projects
                .directory
                .join("claim-installs")
                .join(project.as_str()),
        )
        .await?;
    Ok(observer)
}
