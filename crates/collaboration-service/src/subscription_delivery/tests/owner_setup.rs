//! Real central setup ends before subscription time advances.
use super::*;

impl OwnerFixture {
    pub async fn new() -> Self {
        Box::pin(Self::with_reader(reader_identity())).await
    }
    pub async fn with_reader(reader: Identity) -> Self {
        Box::pin(super::owner_clock::complete_real_setup(async {
            let directory = tempfile::tempdir().unwrap();
            let registry = crate::test_project_runtime::registry(directory.path()).unwrap();
            let origin = ActorOrigin {
                installation: ServiceId::try_from(SERVICE_ID.to_owned()).unwrap(),
                machine: MachineLabel::try_from("Owner-test-machine".to_owned()).unwrap(),
            };
            let (requests, fleet_runtime) =
                board_central_test_support::machine_session(directory.path(), &origin)
                    .await
                    .unwrap();
            let admission = Arc::new(
                ProjectAdmission::new(ProjectAdmissionProps {
                    registry: registry.clone(),
                    fleet: Arc::new(
                        FleetClient::open(
                            directory.path(),
                            Arc::new(crate::test_project_runtime::PendingRegistration),
                            Some(requests),
                        )
                        .await
                        .unwrap(),
                    ),
                    identity: crate::MachineIdentity::new(
                        collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned())
                            .unwrap(),
                        Some("Owner-test-machine"),
                    )
                    .unwrap(),
                })
                .unwrap(),
            );
            let automation = Arc::new(Mutex::new(
                automation_storage::AutomationStore::open(
                    &directory.path().join("automation.sqlite"),
                )
                .await
                .unwrap(),
            ));
            let interactions = Arc::new(
                interaction_storage::InteractionStore::open(
                    directory.path().join("interaction.sqlite"),
                )
                .await
                .unwrap(),
            );
            let clock = Arc::new(TestClock::new(Utc::now()));
            let workspace = WorkspaceId::generate();
            let board = BoardId::generate();
            let project = ProjectId::generate();
            let application = (crate::ApplicationFixture::new(
                SERVICE_ID,
                "00000000-0000-4000-8000-000000000002",
            )
            .unwrap()
            .with_project_admission(admission.clone()))
            .build();
            let header = || {
                json!({"requestId":RequestId::generate(),"protocolVersion":1,
                "actingFor":null,"actor":human("board-owner")})
            };
            let mut request = header();
            request["workspaceId"] = json!(workspace);
            request["name"] = json!("Owner workspace");
            request["description"] = json!("");
            assert_public_accepted(
                &application
                    .board()
                    .workspace_create(serde_json::from_value(request).unwrap())
                    .await
                    .unwrap(),
            );
            let mut request = header();
            request["workspaceId"] = json!(workspace);
            request["boardId"] = json!(board);
            request["name"] = json!("Owner board");
            request["description"] = json!("");
            request["kind"] = json!({"kind":"area"});
            assert_public_accepted(
                &application
                    .board()
                    .board_create(serde_json::from_value(request).unwrap())
                    .await
                    .unwrap(),
            );
            let mut request = header();
            request["boardId"] = json!(board);
            request["projectId"] = json!(project);
            request["title"] = json!("Owner project");
            let request = serde_json::from_value(request).unwrap();
            crate::test_project_runtime::prepare_registered_creation(&admission, &request)
                .await
                .unwrap();
            assert_accepted(&admission.create_project(&request).await.unwrap());
            let discussion_subject = crate::test_subject_workstream::create(&registry, &project)
                .await
                .unwrap();
            let fixture = Self {
                discussion_subject,
                store: Arc::new(Mutex::new(
                    admission.subscription_window_store(interactions.clone()),
                )),
                directory,
                admission,
                registry,
                automation,
                interactions,
                reader,
                project,
                root: ThreadId::generate(),
                clock,
                presence: Arc::new(ScriptedPresence(StdMutex::new(TargetPresence::Running))),
            };
            fixture
                .create_root(&fixture.root, human("board-owner"))
                .await;
            fixture
                .join(
                    &fixture.root,
                    fixture.reader.clone(),
                    ParticipantRole::Participant,
                    true,
                    None,
                )
                .await;
            fixture
                .policy(
                    &fixture.root,
                    if matches!(fixture.reader, Identity::Human { .. }) {
                        SubscriptionMode::Poll
                    } else {
                        SubscriptionMode::Deliver
                    },
                    WhenIdle::Hold,
                    0,
                    0,
                )
                .await;
            fleet_runtime.shutdown().await.unwrap();
            fixture
        }))
        .await
    }

    /// Reopen the real setup owner only while arranging another native project.
    pub async fn prepare_additional_project(&self, request: &ProjectCreateRequest) {
        Box::pin(super::owner_clock::complete_real_setup(async {
            let (requests, runtime) = board_central_test_support::machine_session(
                self.directory.path(),
                &self.admission.record_origin(),
            )
            .await
            .unwrap();
            // Unit tests construct their own crate type with the shared session;
            // the support crate's compiled FleetClient is a distinct Rust type.
            let fleet = Arc::new(
                FleetClient::open(
                    self.directory.path(),
                    Arc::new(crate::test_project_runtime::PendingRegistration),
                    Some(requests),
                )
                .await
                .unwrap(),
            );
            let admission = ProjectAdmission::new(ProjectAdmissionProps {
                registry: self.registry.clone(),
                fleet: fleet.clone(),
                identity: crate::MachineIdentity::new(
                    collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
                    Some("Owner-test-machine"),
                )
                .unwrap(),
            })
            .unwrap();
            crate::test_project_runtime::prepare_registered_creation(&admission, request)
                .await
                .unwrap();
            drop(admission);
            runtime.shutdown().await.unwrap();
            Arc::try_unwrap(fleet)
                .unwrap_or_else(|_| panic!("additional-project setup still owns its Fleet client"))
                .close()
                .await;
        }))
        .await;
    }
}
