//! Cold recovery skips persisted terminal P before opening its retained writer.
use super::*;
use fleet_sync_protocol::{CommitSequence, LeaseRefusal, RenewAnswer, ReplicationPosition};
use runtime_primitives::{Clock, SystemClock};

#[tokio::test]
async fn cold_welcome_and_notice_recovery_skip_superseded_project_and_initialize_next() -> TestResult
{
    // Arrange P, then Q's actual accepted directory input at the crash interval
    // between directory settlement and native file creation.
    let runtime = Box::pin(Runtime::new()).await?;
    let context = runtime.admission.runtime_test_context();
    let mut creation = runtime
        .fleet
        .accepted_creation_page("", 100)
        .await?
        .into_iter()
        .find(|request| request.project_id == runtime.project)
        .ok_or("P creation")?;
    creation.project_id = ProjectId::generate();
    creation.request_id = RequestId::generate();
    creation.title = "Q after superseded P".to_owned().try_into()?;
    assert_eq!(
        runtime.admission.create_project(&creation).await?.result,
        RequestResult::PendingUnreachable
    );
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let retained = writer.store.path().to_owned();
    context
        .registry
        .database(&runtime.project)
        .await?
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
                current_epoch: Epoch::new(2)?,
                epoch_ended_at: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)),
            }),
            SystemClock.now(),
        )
        .await?;
    let source = Arc::downgrade(&writer.store);
    drop(writer);
    runtime
        .projects
        .supersede_project(&runtime.project, source)
        .await?;
    let Runtime {
        directory,
        mut central,
        admission,
        fleet,
        projects,
        requests,
        running,
        project,
        ..
    } = runtime;
    running.shutdown().await?;
    // Settlement commits before the stopped Host refuses registration. This is
    // the real durable state, with no fabricated directory or registration row.
    assert!(settle_fixture_directory(&fleet, &creation).await.is_err());
    assert!(
        fleet
            .accepted_creation_page("", 100)
            .await?
            .iter()
            .any(|input| input == &creation)
    );
    let q_directory = directory
        .path()
        .join("router/projects")
        .join(creation.project_id.as_str());
    assert!(
        !q_directory.exists(),
        "Q already had a native file before cold recovery"
    );
    drop(context);
    drop(fleet);
    drop(projects);
    drop(requests);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("old admission retained"))?
        .close()
        .await?;
    let retained_before = durable_file_hashes(&retained)?;

    // Act with freshly opened owners and the same actual central session target.
    let board = open_board_admission(
        &directory.path().join("router"),
        identity()?,
        Some(central.inputs()?),
    )
    .await?;
    let session = board.session.as_ref().ok_or("resumed session")?;
    let projects = session.projects.clone();
    let requests = session.requests.clone();
    let ready = session.welcomed.subscribe();
    let admission = board.admission.clone();
    let fleet = board.fleet.upgrade().ok_or("resumed fleet")?;
    let running = board.start().ok_or("resumed start")?;
    wait_welcome(ready).await?;
    timeout(BOUND, async {
        loop {
            match admission.create_project(&creation).await {
                Ok(reply)
                    if matches!(
                        reply.result,
                        RequestResult::Decided(RequestDecision::Accepted { .. })
                    ) =>
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                Ok(_) => {}
                Err(collaboration_service::ProjectAdmissionError::Registry(
                    collaboration_service::ProjectRegistryError::Unavailable { project },
                )) if project == creation.project_id => {}
                Err(error) => return Err(error.into()),
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| {
        io::Error::other("cold Welcome recovery did not initialize Q after terminal P")
    })??;
    assert!(q_directory.join("e1.sqlite").exists());
    assert_eq!(
        collaboration_service::ProjectRegistration::start(projects.as_ref(), &project).await?,
        ProjectRegistrationState::Superseded
    );

    // Both cursor recovery branches, followed by P's old hint then an unrelated
    // hint, must save machine progress without reopening the terminal writer.
    central.wait_notice_reads(2).await?;
    let progress_root = directory.path().join("cold-component-progress");
    let mut reactions = MachineNoticeReactions::open(&progress_root).await?;
    central
        .script_notice_answers(vec![
            NoticeRead::CursorExpired {
                floor: NoticePosition::new(4),
            },
            page(vec![
                PositionedBoardNotice {
                    position: NoticePosition::new(5),
                    kind: BoardNotice::LeaseSuperseded {
                        project: project.clone(),
                        epoch: Epoch::new(1)?,
                    },
                },
                PositionedBoardNotice {
                    position: NoticePosition::new(6),
                    kind: BoardNotice::FleetFeedAdvanced {
                        through: FleetPosition::ZERO,
                    },
                },
            ]),
            page(Vec::new()),
        ])
        .await;
    reactions
        .catch_up_notices(
            &projects,
            &Arc::downgrade(&admission),
            &Arc::downgrade(&fleet),
            &std::sync::Weak::new(),
            &requests,
        )
        .await?;
    assert_eq!(
        MachineNoticeProgressStore::open(&progress_root)
            .await?
            .load()
            .await?,
        NoticePosition::new(6)
    );
    central
        .script_notice_answers(vec![
            NoticeRead::CursorInvalid,
            page(vec![
                PositionedBoardNotice {
                    position: NoticePosition::new(1),
                    kind: BoardNotice::LeaseSuperseded {
                        project: project.clone(),
                        epoch: Epoch::new(1)?,
                    },
                },
                PositionedBoardNotice {
                    position: NoticePosition::new(2),
                    kind: BoardNotice::FleetFeedAdvanced {
                        through: FleetPosition::ZERO,
                    },
                },
            ]),
            page(Vec::new()),
        ])
        .await;
    reactions
        .catch_up_notices(
            &projects,
            &Arc::downgrade(&admission),
            &Arc::downgrade(&fleet),
            &std::sync::Weak::new(),
            &requests,
        )
        .await?;
    assert_eq!(
        MachineNoticeProgressStore::open(&progress_root)
            .await?
            .load()
            .await?,
        NoticePosition::new(2)
    );
    assert!(projects.writer_is_terminal(&project).await?);
    assert_eq!(durable_file_hashes(&retained)?, retained_before);
    drop(fleet);
    drop(requests);
    running.shutdown().await?;
    drop(projects);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("new admission retained"))?
        .close()
        .await?;
    central.stop().await?;
    Ok(())
}
