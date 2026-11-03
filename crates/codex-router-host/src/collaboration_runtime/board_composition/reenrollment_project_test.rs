//! Enrollment clears learned Router state while each real writer asks central again.
use super::*;
use board_central_test_support::TestCentral;

struct Lifetime {
    admission: Arc<ProjectAdmission>,
    projects: Arc<super::super::project_writers::ProjectWriters>,
    running: super::super::machine_session::RunningMachineSession,
}

impl Lifetime {
    async fn open(
        root: &Path,
        central: &TestCentral,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let board =
            open_board_admission(root, identity()?, Some(central.session_inputs()?)).await?;
        let projects = board
            .session
            .as_ref()
            .ok_or("machine session")?
            .projects
            .clone();
        let admission = board.admission.clone();
        let fleet = board.fleet.upgrade().ok_or("Fleet client")?;
        let running = board.start().ok_or("machine runtime")?;
        wait_welcome(projects.welcomed.clone()).await?;
        fleet.synchronize().await?;
        Ok(Self {
            admission,
            projects,
            running,
        })
    }

    async fn create_project(&self) -> Result<ProjectId, Box<dyn std::error::Error + Send + Sync>> {
        let app = collaboration_service::ApplicationFixture::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(self.admission.clone())
        .build();
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let actor = Identity::Human {
            human_id: HumanId::try_from("reenrollment owner".to_owned())?,
        };
        let created = app.board().workspace_create(serde_json::from_value(serde_json::json!({
            "requestId": RequestId::generate(), "protocolVersion": 1, "workspaceId": workspace,
            "name": "reenrollment workspace", "description": "", "actor": actor, "actingFor": null,
        }))?).await?;
        assert!(
            matches!(
                created.result,
                public_replies::RequestResult::Decided(
                    public_replies::RequestDecision::Accepted { .. }
                )
            ),
            "{created:?}"
        );
        let created = app.board().board_create(serde_json::from_value(serde_json::json!({
            "requestId": RequestId::generate(), "protocolVersion": 1, "workspaceId": workspace,
            "boardId": board, "name": "reenrollment board", "description": "", "kind": {"kind":"area"},
            "actor": actor, "actingFor": null,
        }))?).await?;
        assert!(
            matches!(
                created.result,
                public_replies::RequestResult::Decided(
                    public_replies::RequestDecision::Accepted { .. }
                )
            ),
            "{created:?}"
        );
        let request = ProjectCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: ProjectId::generate(),
            board_id: board,
            title: ResourceName::try_from("retained writer".to_owned())?,
            description: None,
            actor,
            acting_for: None,
        };
        self.admission.create_project(&request).await?;
        let writer = self.projects.ensure(&request.project_id).await?;
        assert!(matches!(
            timeout(BOUND, writer.runtime.wait_registration())
                .await
                .map_err(|error| format!(
                    "registration wait {}: {error}",
                    request.project_id.as_str()
                ))??,
            WriterRegistrationOutcome::Ready { .. }
        ));
        let created = self.admission.create_project(&request).await?;
        assert!(
            matches!(
                created.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{created:?}"
        );
        let granted = self
            .admission
            .submit(&ProjectRequest {
                request_id: RequestId::generate(),
                project_id: request.project_id.clone(),
                actor: request.actor.clone(),
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                operation: BoardOperation::RoleGrant {
                    subject: RoleSubject::Project(request.project_id.clone()),
                    role: AuthorityRole::Lead,
                    holder: request.actor,
                    model: None,
                    lineage: None,
                    expected_revision: AssignmentRevision::INITIAL,
                    owner_grant: None,
                },
            })
            .await?;
        assert!(
            matches!(
                granted.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{granted:?}"
        );
        Ok(request.project_id)
    }

    async fn write(
        &self,
        project: &ProjectId,
        text: &str,
    ) -> Result<message_board::WriteReply, Box<dyn std::error::Error + Send + Sync>> {
        let store = self
            .admission
            .project_registry()
            .existing_project(project)
            .await?
            .ok_or("writer file")?;
        let mut snapshot = store.snapshot().await?;
        let Some(Record::Project(state)) = snapshot.record(&project.clone().into()).await? else {
            return Err("project row missing".into());
        };
        snapshot.finish().await?;
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: project.clone(),
            actor: Identity::Human {
                human_id: HumanId::try_from("reenrollment owner".to_owned())?,
            },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: BoardOperation::ProjectDescribe {
                description: Description::try_from(text.to_owned())?,
                expected_revision: state.revision,
            },
        };
        Ok(self.admission.submit(&request).await?)
    }

    async fn await_copy(&self, project: &ProjectId, central: &TestCentral) -> TestResult {
        let store = self
            .admission
            .project_registry()
            .existing_project(project)
            .await?
            .ok_or("writer file")?;
        let snapshot = store.snapshot().await?;
        let head = snapshot.high_water();
        snapshot.finish().await?;
        timeout(BOUND, async {
            loop {
                if central.copy_status(project).await?.acked.sequence.value() >= head.position.get()
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|error| format!("copy acknowledgement {}: {error}", project.as_str()))??;
        Ok(())
    }

    async fn close(self) -> TestResult {
        self.running.shutdown().await?;
        drop(self.projects);
        Arc::try_unwrap(self.admission)
            .map_err(|_| io::Error::other("admission still owned"))?
            .close()
            .await?;
        Ok(())
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn reenrollment_with_an_existing_writer_renews_and_writes_against_unchanged_central()
-> TestResult {
    // Arrange a real granted writer whose original history already reached central.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let central = TestCentral::start(&directory.path().join("central"), &origin()?).await?;
    let original = Lifetime::open(&root, &central).await?;
    let project = original.create_project().await?;
    let written = original.write(&project, "before reenrollment").await?;
    assert!(
        matches!(
            written.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{written:?}"
    );
    original.await_copy(&project, &central).await?;
    original.close().await?;

    // Act: exercise the same durable reset obligation the enrollment publisher records.
    drop(crate::enrollment_reset::record_pending_enrollment_reset(
        &root,
    )?);
    let reopened = Lifetime::open(&root, &central).await?;
    reopened.projects.prompt_project_renewal(&project).await?;
    let writer = reopened.projects.ensure(&project).await?;
    let mut changes = reopened
        .admission
        .project_registry()
        .database(&project)
        .await?
        .subscribe_authority_changes();
    let refusal = timeout(BOUND, async {
        loop {
            if matches!(
                writer.store.writer_state().await?,
                fleet_sync_lease::WriterState::Writing { .. }
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            changes.changed().await?;
        }
    })
    .await;
    if let Err(error) = refusal {
        return Err(format!(
            "unchanged-central renewal: {error}; state={:?}",
            writer.store.writer_state().await
        )
        .into());
    }
    refusal??;
    drop(writer);
    // Assert a new real admission and Copy succeed without rebuilding the file.
    let written = reopened.write(&project, "after reenrollment").await?;
    assert!(
        matches!(
            written.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{written:?}"
    );
    reopened.await_copy(&project, &central).await?;
    reopened.close().await?;
    central.shutdown().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn wiped_central_leaves_only_the_unknown_project_read_only_while_new_creation_works()
-> TestResult {
    // Arrange a real writer and its replicated history before central loses its corpus.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let central = TestCentral::start(&directory.path().join("central"), &origin()?).await?;
    let original = Lifetime::open(&root, &central).await?;
    let project = original.create_project().await?;
    original.await_copy(&project, &central).await?;
    let store = original
        .admission
        .project_registry()
        .existing_project(&project)
        .await?
        .ok_or("original writer")?;
    let snapshot = store.snapshot().await?;
    let head = snapshot.high_water();
    snapshot.finish().await?;
    drop(store);
    original.close().await?;
    central.shutdown().await?;

    // Act: a fresh real corpus uses the same configured machine and receiver ids.
    let wiped = TestCentral::start(&directory.path().join("wiped-central"), &origin()?).await?;
    drop(crate::enrollment_reset::record_pending_enrollment_reset(
        &root,
    )?);
    let reopened = Lifetime::open(&root, &wiped).await?;
    let writer = reopened.projects.ensure(&project).await?;
    let mut changes = reopened
        .admission
        .project_registry()
        .database(&project)
        .await?
        .subscribe_authority_changes();
    reopened.projects.prompt_project_renewal(&project).await?;
    let refusal = timeout(BOUND, async {
        loop {
            if matches!(
                writer.store.writer_state().await,
                Err(message_board_storage::ProjectStorageError::Writer(
                    fleet_sync_writer::OpenError::Holder {
                        error: fleet_sync_lease::HolderError::Refused(
                            fleet_sync_protocol::LeaseRefusal::NotRegistered
                        )
                    }
                ))
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            changes.changed().await?;
        }
    })
    .await;
    if let Err(error) = refusal {
        return Err(format!(
            "wiped-central authoritative refusal: {error}; state={:?}",
            writer.store.writer_state().await
        )
        .into());
    }
    refusal??;
    drop(writer);

    // Assert the existing native snapshot remains readable with its exact prior head.
    let registry = reopened.admission.project_registry();
    let reader = registry
        .read_project(&project)
        .await?
        .ok_or("retained local reader")?;
    let snapshot = reader.snapshot().await?;
    assert_eq!(snapshot.high_water(), head);
    let (status, _, _) = snapshot.view_context().ok_or("local project view status")?;
    assert_eq!(
        serde_json::to_value(status)?,
        serde_json::json!({"kind":"notInCentral"})
    );
    snapshot.finish().await?;
    drop(reader);
    drop(registry);
    let refused = reopened.write(&project, "must remain read only").await?;
    assert!(
        matches!(refused.result, RequestResult::NotWriter { current: None }),
        "{refused:?}"
    );
    let new_project = reopened.create_project().await?;
    let written = reopened
        .write(&new_project, "another project still writes")
        .await?;
    assert!(
        matches!(
            written.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{written:?}"
    );
    reopened.await_copy(&new_project, &wiped).await?;
    reopened
        .close()
        .await
        .map_err(|error| format!("unknown-project machine shutdown: {error}"))?;
    wiped.shutdown().await?;
    Ok(())
}
