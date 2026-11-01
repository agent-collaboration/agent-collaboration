//! A sender's board entry uses the real machine session, central and holder ledger.
use super::*;
use board_central_test_support::EnrolledTestMachine;
use collaboration_service::{ApplicationFixture, CollaborationApplication};
use message_board::public_replies::WriteReply;

#[cfg(feature = "runtime-test-support")]
#[path = "non_holder_task_writes.rs"]
mod non_holder_task_writes;
#[cfg(feature = "runtime-test-support")]
#[path = "forwarded_post_notifications.rs"]
mod notifications;
#[path = "forwarded_post_unknown.rs"]
mod unknown_answer;

struct SenderFixture {
    application: CollaborationApplication,
    admission: Arc<ProjectAdmission>,
    running: Option<RunningMachineSession>,
    machine: EnrolledTestMachine,
    origin: ActorOrigin,
    directory: std::path::PathBuf,
}

impl SenderFixture {
    async fn start(
        holder: &claim_fixture::ClaimFixture,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let origin = ActorOrigin {
            installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440988".to_owned())?,
            machine: MachineLabel::try_from("forwarding sender fixture".to_owned())?,
        };
        let machine = holder
            .central
            .additional_machine(
                &origin,
                fleet_sync_protocol::PeerId::new(durable_steps::Uuid::now_v7())?,
            )
            .await?;
        let directory = holder.directory.path().join("sender");
        let board = open_board_admission(
            &directory,
            MachineIdentity::new(
                UuidIdentity::try_from(origin.installation.as_str().to_owned())?,
                Some(origin.machine.as_str()),
            )?,
            Some(machine.session_inputs()?),
            None,
        )
        .await?;
        let welcomed = board
            .session
            .as_ref()
            .ok_or("sender session")?
            .welcomed
            .subscribe();
        let admission = board.admission.clone();
        let running = board.start().ok_or("sender runtime")?;
        wait_welcome(welcomed).await?;
        let application = (ApplicationFixture::new(
            origin.installation.as_str(),
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone()))
        .build();
        Ok(Self {
            application,
            admission,
            running: Some(running),
            machine,
            origin,
            directory,
        })
    }

    async fn disconnect(&mut self) -> TestResult {
        if let Some(running) = self.running.take() {
            running.shutdown().await?;
        }
        Ok(())
    }

    async fn shutdown(mut self) -> TestResult {
        let stopped = self.disconnect().await;
        drop(self.application);
        let closed = Arc::try_unwrap(self.admission)
            .map_err(|_| io::Error::other("sender admission still owned"))?
            .close()
            .await;
        let edge = self.machine.shutdown().await;
        stopped?;
        closed?;
        edge?;
        Ok(())
    }
}

fn post_request(
    project: &ProjectId,
) -> Result<ProjectRequest, Box<dyn std::error::Error + Send + Sync>> {
    // A declared actor is independent of the machine's verified provenance.
    Ok(serde_json::from_value(serde_json::json!({
        "requestId": RequestId::generate(), "projectId": project, "protocolVersion": 1,
        "actor": {"kind":"session","session":{"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440999","endpointId":"codex-local"},"sessionId":"forwarding-declared-session"}}, "actingFor": null,
        "operation": {"kind":"threadCreate","postId": PostId::generate(),
            "about":{"kind":"project","subject":{"line":"forwarding fixture"}},
            "text":"synthetic forwarding fixture", "references":[], "participation":"participant",
            "watch":false,"addressees":[]}
    }))?)
}

fn result_kind(reply: &WriteReply) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(reply)?;
    Ok(value
        .pointer("/result/kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned())
}

async fn grant_posting_role(
    holder: &claim_fixture::ClaimFixture,
    request: &ProjectRequest,
) -> TestResult {
    let grant = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: request.project_id.clone(),
        actor: request.actor.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::RoleGrant {
            subject: RoleSubject::Project(request.project_id.clone()),
            role: AuthorityRole::Lead,
            holder: request.actor.clone(),
            model: None,
            lineage: None,
            expected_revision: AssignmentRevision::INITIAL,
            owner_grant: None,
        },
    };
    assert!(
        matches!(
            holder.admission.submit(&grant).await?.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "fixture must acquire its role through real holder admission"
    );
    Ok(())
}

async fn claimed_project(
    fixture: &claim_fixture::ClaimFixture,
) -> Result<ProjectId, Box<dyn std::error::Error + Send + Sync>> {
    let project = ProjectId::generate();
    project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    assert!(matches!(
        fixture.claim(&project).await?,
        board_sync_protocol::ProjectClaimReply::Claimed { .. }
    ));
    Ok(project)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_admits_both_post_families_and_replays_at_the_holder() -> TestResult {
    let holder = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = claimed_project(&holder).await?;
    let sender = Box::pin(SenderFixture::start(&holder)).await?;
    let proof = Box::pin(async {
        let request = post_request(&project)?;
        grant_posting_role(&holder, &request).await?;
        let BoardOperation::ThreadCreate {
            post_id,
            about,
            text,
            participation,
            watch,
            ..
        } = &request.operation
        else {
            return Err("create fixture operation".into());
        };
        let create = ThreadCreateRequest {
            request_id: request.request_id.clone(),
            protocol_version: request.protocol_version,
            post_id: post_id.clone(),
            project_id: project.clone(),
            about: about.clone(),
            actor: request.actor.clone(),
            acting_for: request.acting_for.clone(),
            text: text.clone(),
            references: serde_json::from_value(serde_json::json!([]))?,
            addressees: Vec::new(),
            role: *participation,
            watch: *watch,
        };
        let created = sender
            .application
            .board()
            .thread_create(create.clone())
            .await?;
        assert_eq!(
            result_kind(&created)?,
            "decided",
            "non-holder post must reach holder admission"
        );
        assert!(
            matches!(
                &created.result,
                message_board::public_replies::RequestResult::Decided(
                    message_board::public_replies::RequestDecision::Accepted { .. }
                )
            ),
            "eligible forwarded root must be accepted"
        );
        assert!(
            created.reader.is_none(),
            "holder reader position is not a sender read-copy observation"
        );
        let BoardOperation::ThreadCreate { post_id, .. } = &request.operation else {
            return Err("create fixture operation".into());
        };
        let store = holder
            .registry
            .existing_project(&project)
            .await?
            .ok_or("holder store")?;
        let mut read = store.snapshot().await?;
        let root_record = read
            .record(&post_id.clone().into())
            .await?
            .ok_or("forwarded root record")?;
        assert_eq!(
            root_record.envelope().actor,
            RecordedActor::declared(request.actor.clone())
        );
        let RecordPlacement::Project(entry) = &root_record.envelope().position else {
            return Err("forwarded root placement".into());
        };
        assert_eq!(
            entry.origin, sender.origin,
            "central must bind the sender's enrolled installation"
        );
        let ledger = read
            .request_effects(&message_board::RequestEffectsRequest {
                scope: RequestEffectsScope::Project {
                    project_id: project.clone(),
                },
                request: request.request_id.clone(),
                after: None,
            })
            .await?;
        let RequestEffectsRead::Decision {
            decision: holder_decision,
        } = ledger
        else {
            return Err("holder's immutable decision receipt".into());
        };
        let message_board::public_replies::RequestResult::Decided(returned_decision) =
            &created.result
        else {
            return Err("returned holder decision".into());
        };
        assert_eq!(
            serde_json::to_value(returned_decision)?,
            serde_json::to_value(holder_decision)?,
            "sender changed the holder's recorded decision"
        );
        let before_replay = read.high_water();
        read.finish().await?;
        let replay = sender.application.board().thread_create(create).await?;
        assert_eq!(replay.result, created.result);
        let read = store.snapshot().await?;
        assert_eq!(
            read.high_water(),
            before_replay,
            "retry appended another record"
        );
        read.finish().await?;

        let reply_id = PostId::generate();
        let reply = sender
            .application
            .board()
            .thread_post(ThreadPostRequest {
                request_id: RequestId::generate(),
                protocol_version: request.protocol_version,
                post_id: reply_id.clone(),
                placement: PostTarget::Thread {
                    thread: ThreadRef {
                        project: Some(project.clone()),
                        thread: post_id.clone().into(),
                    },
                },
                actor: request.actor.clone(),
                acting_for: request.acting_for.clone(),
                text: PostText::try_from("synthetic forwarded reply".to_owned())?,
                references: serde_json::from_value(serde_json::json!([]))?,
                addressees: Vec::new(),
            })
            .await?;
        assert!(
            matches!(
                reply.result,
                message_board::public_replies::RequestResult::Decided(
                    message_board::public_replies::RequestDecision::Accepted { .. }
                )
            ),
            "eligible forwarded reply must be accepted"
        );
        let mut read = store.snapshot().await?;
        assert!(
            read.record(&reply_id.into()).await?.is_some(),
            "forwarded reply was not admitted"
        );
        let before_non_post = read.high_water();
        read.finish().await?;
        let mut non_post = request;
        non_post.request_id = RequestId::generate();
        non_post.operation = BoardOperation::ProjectDescribe {
            description: Description::try_from("synthetic forbidden non-holder edit".to_owned())?,
            expected_revision: SubjectRevision::INITIAL,
        };
        let refusal = sender
            .application
            .board()
            .project_write(non_post.clone())
            .await?;
        assert!(matches!(
            result_kind(&refusal)?.as_str(),
            "notWriter" | "scopeUnavailable"
        ));
        let mut read = store.snapshot().await?;
        assert_eq!(
            read.high_water(),
            before_non_post,
            "a non-post write reached the holder"
        );
        assert!(
            read.record(&non_post.request_id.into()).await?.is_none(),
            "non-post request entered the holder ledger"
        );
        read.finish().await?;
        #[cfg(feature = "runtime-test-support")]
        assert!(
            sender
                .admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty(),
            "forwarding entered the directory outbox"
        );
        assert!(
            !sender
                .directory
                .join("projects")
                .join(project.as_str())
                .exists(),
            "sender created a local writer"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await;
    let sender_closed = sender.shutdown().await;
    let holder_closed = holder.shutdown().await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_reports_offline_holder_presence_without_queuing() -> TestResult {
    let mut holder = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = claimed_project(&holder).await?;
    let sender = Box::pin(SenderFixture::start(&holder)).await?;
    holder.disconnect_holder().await?;
    let proof = async {
        let preview: serde_json::Value = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .post(format!(
                "{}/admin/v1/projects/{}/takeover/preview",
                holder.central.base_url(),
                project.as_str()
            ))
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::owner_assertion()?,
            )
            .json(&serde_json::Value::Null)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let observed: Option<fleet_sync_protocol::SyncTime> = serde_json::from_value(
            preview
                .get("lastSeen")
                .cloned()
                .ok_or("owner presence observation")?,
        )?;
        let expected = chrono::DateTime::from_timestamp_millis(
            observed
                .ok_or("holder must have a real presence observation")?
                .unix_millis(),
        )
        .ok_or("fixture time range")?;
        let reply = sender
            .application
            .board()
            .project_write(post_request(&project)?)
            .await?;
        assert_eq!(result_kind(&reply)?, "holderUnreachable");
        assert!(
            matches!(
                &reply.result,
                message_board::public_replies::RequestResult::HolderUnreachable {
                    last_seen: Some(actual)
                } if *actual == expected
            ),
            "forwarded refusal must retain the independently read presence time"
        );
        let value = serde_json::to_value(reply)?;
        assert!(
            value
                .pointer("/result/decision/lastSeen")
                .is_some_and(|value| !value.is_null()),
            "offline presence must retain central's last-seen observation"
        );
        #[cfg(feature = "runtime-test-support")]
        assert!(
            sender
                .admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty(),
            "forwarding entered the directory outbox"
        );
        assert!(
            !sender
                .directory
                .join("projects")
                .join(project.as_str())
                .exists()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let sender_closed = sender.shutdown().await;
    let holder_closed = holder.shutdown().await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_reports_an_imported_unheld_project() -> TestResult {
    let holder = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    project_claim::imported_snapshot(
        &holder.central,
        &holder.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let sender = Box::pin(SenderFixture::start(&holder)).await?;
    let proof = async {
        let reply = sender
            .application
            .board()
            .project_write(post_request(&project)?)
            .await?;
        assert_eq!(result_kind(&reply)?, "notHeld");
        #[cfg(feature = "runtime-test-support")]
        assert!(
            sender
                .admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty(),
            "forwarding entered the directory outbox"
        );
        assert!(
            !sender
                .directory
                .join("projects")
                .join(project.as_str())
                .exists()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let sender_closed = sender.shutdown().await;
    let holder_closed = holder.shutdown().await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_without_a_machine_session_refuses_without_queuing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let board = open_board_admission(directory.path(), identity()?, None, None).await?;
    let admission = board.admission.clone();
    assert!(board.start().is_none());
    let application = (ApplicationFixture::new(
        origin()?.installation.as_str(),
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone()))
    .build();
    let proof = async {
        let reply = application
            .board()
            .project_write(post_request(&ProjectId::generate())?)
            .await?;
        assert!(matches!(
            reply.result,
            message_board::public_replies::RequestResult::PendingUnreachable
        ));
        assert!(reply.reader.is_none());
        #[cfg(feature = "runtime-test-support")]
        assert!(
            admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(application);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("unattached admission still owned"))?
        .close()
        .await;
    proof?;
    closed?;
    Ok(())
}
