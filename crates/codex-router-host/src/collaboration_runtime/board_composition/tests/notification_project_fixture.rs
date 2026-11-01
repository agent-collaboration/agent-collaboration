//! Seed a real unacknowledged notification, then close every original file owner.
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub(in crate::collaboration_runtime::board_composition) struct NotificationSeed {
    pub project: ProjectId,
    pub item: ItemId,
    pub epoch: EpochNumber,
}

pub(in crate::collaboration_runtime::board_composition) async fn seed_unacknowledged_notification(
    root: &Path,
    central: &mut TestFleet,
) -> Result<NotificationSeed, Box<dyn std::error::Error + Send + Sync>> {
    let enrollment = IntentRecipientEnrollment {
        service: "00000000-0000-4000-8000-000000000099"
            .to_owned()
            .try_into()?,
        peer: PeerId::new(durable_steps::Uuid::now_v7())?,
    };
    Box::pin(seed_notification_for_recipient(root, central, enrollment)).await
}

pub(in crate::collaboration_runtime::board_composition) async fn seed_unacknowledged_local_notification(
    root: &Path,
    central: &mut TestFleet,
) -> Result<NotificationSeed, Box<dyn std::error::Error + Send + Sync>> {
    let enrollment = IntentRecipientEnrollment {
        service: "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        peer: central.peer,
    };
    Box::pin(seed_notification_for_recipient(root, central, enrollment)).await
}

async fn seed_notification_for_recipient(
    root: &Path,
    central: &mut TestFleet,
    enrollment: IntentRecipientEnrollment,
) -> Result<NotificationSeed, Box<dyn std::error::Error + Send + Sync>> {
    let machine = MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        Some("notice fixture"),
    )?;
    let local_origin = ActorOrigin {
        installation: String::from(machine.service_id().clone()).try_into()?,
        machine: machine.machine_label().as_str().to_owned().try_into()?,
    };
    let board = open_board_admission(root, machine, Some(central.inputs()?), None).await?;
    let projects = board.session.as_ref().ok_or("session")?.projects.clone();
    let welcomed = board
        .session
        .as_ref()
        .ok_or("session")?
        .welcomed
        .subscribe();
    let replication = board.replication.as_ref().ok_or("replication")?.clone();
    let (fleet, creating) = arrange_project_input(&board, root).await?;
    // This seed needs the durable creating input, not a transport timeout before
    // the session pump starts. Poll the real admission until its outbox commit,
    // then drop the caller while the same durable request remains recoverable.
    {
        let create = board.admission.create_project(&creating);
        tokio::pin!(create);
        timeout(BOUND, async {
            loop {
                tokio::select! {
                    reply = &mut create => {
                        assert_eq!(reply?.result, RequestResult::PendingUnreachable);
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    queued = fleet.creation_input(&creating.request_id) => {
                        if queued? == Some(creating.clone()) {
                            return Ok(());
                        }
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
    }
    let admission = board.admission.clone();
    let running = board.start().ok_or("session start")?;
    timeout(BOUND, central.auth.entered.notified()).await?;
    settle_fixture_directory(&fleet, &creating).await?;
    central.release()?;
    wait_welcome(welcomed).await?;
    timeout(BOUND, central.registration_gate.entered.notified()).await?;
    let writer = projects.ensure(&creating.project_id).await?;
    timeout(BOUND, writer.runtime.wait_registration()).await??;
    timeout(BOUND, async {
        loop {
            let mut read = writer.store.snapshot().await?;
            let exists = read
                .record(&creating.project_id.clone().into())
                .await?
                .is_some();
            read.finish().await?;
            if exists {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let workstream = WorkstreamId::generate();
    let mut write = writer.store.begin_admission(local_origin).await?;
    let Some(Record::Project(state)) = write.record(&creating.project_id.clone().into()).await?
    else {
        return Err("project".into());
    };
    // Subject grant policy is a fixture input; the notification producer is real.
    write
        .append_record(Record::Workstream(Workstream {
            envelope: RecordEnvelope {
                id: workstream.clone().into(),
                request: None,
                ..state.envelope
            },
            workstream_id: workstream.clone(),
            project_id: creating.project_id.clone(),
            name: "notification subject".to_owned().try_into()?,
            goal: "Welcome recovery".to_owned(),
            state: WorkstreamState::Open {},
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    write.commit().await?;
    let post = PostId::generate();
    submit_operation(
        &admission,
        &creating,
        BoardOperation::ThreadCreate {
            post_id: post.clone(),
            about: ThreadSubject::Workstream(workstream),
            text: "notification across reconnect".to_owned().try_into()?,
            references: Vec::new(),
            participation: None,
            watch: false,
            addressees: vec![NotifyTarget::Actor(Identity::Session {
                session: SessionRef {
                    endpoint: SessionEndpointRef {
                        service_id: enrollment.service.clone(),
                        endpoint_id: "codex-local".to_owned().try_into()?,
                    },
                    session_id: "remote-notification-reader".to_owned().try_into()?,
                },
            })],
        },
    )
    .await?;
    let mut read = writer.store.snapshot().await?;
    let head = read.high_water();
    let intent = read
        .records(RecordKind::NotifyIntent)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => {
                Some(intent.intent_id)
            }
            _ => None,
        })
        .ok_or("intent")?;
    read.finish().await?;
    timeout(BOUND, async {
        loop {
            projects
                .replicate_project_for_test(&creating.project_id)
                .await?;
            if replication
                .replicated_through(&creating.project_id, head.epoch)
                .await?
                .position
                >= head.position
            {
                return TestResult::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    writer.stop().await?;
    assert!(matches!(
        writer.store.writer_state().await?,
        fleet_sync_lease::WriterState::Writing { .. }
    ));
    let item = ItemId::new(durable_steps::Uuid::now_v7())?;
    admission
        .claim_intent(
            &creating.project_id,
            &intent,
            Some(IntentHandoffInput { item, enrollment }),
        )
        .await?;
    assert!(
        replication
            .acknowledged_mail_changes(&creating.project_id)
            .await?
            .max_handoff_seq
            .is_none()
    );
    drop(writer);
    running.shutdown().await?;
    drop(fleet);
    drop(projects);
    drop(replication);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("seed admission retained"))?
        .close()
        .await?;
    Ok(NotificationSeed {
        project: creating.project_id,
        item,
        epoch: head.epoch,
    })
}
