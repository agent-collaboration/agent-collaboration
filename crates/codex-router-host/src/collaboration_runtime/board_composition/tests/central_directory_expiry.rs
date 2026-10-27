//! Native Directory time changes must remain healthy through the Host's A9 reader.
use super::*;
use board_central_test_support::{ObjectAddress, RuntimeControl};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardReadPage, BoardReadScope,
    ProjectClaimReply,
};
use fleet_admission::{
    DirectoryEntry, DirectoryLease, DirectoryLeaseState, DirectoryQuery, DirectorySide,
    LeaseMetadata,
};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use std::os::unix::fs::DirBuilderExt;

async fn directory_lease(
    requests: &fleet_sync_tokio_runner::SessionRequests<board_sync_protocol::BoardConsumerProtocol>,
    project: &ProjectId,
) -> Result<LeaseMetadata, Box<dyn std::error::Error + Send + Sync>> {
    timeout(BOUND, async {
        loop {
            let answer = requests
                .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope: BoardReadScope::Directory {
                        query: BoardPayload::encode(&DirectoryQuery::default())?,
                    },
                    cursor: None,
                }))
                .await?;
            let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
                entries,
                ..
            })) = answer
            else {
                return Err("native Directory returned another page family".into());
            };
            for entry in entries {
                let entry: DirectoryEntry = entry.decode()?;
                if entry.project == *project
                    && let DirectorySide::Known(lease) = entry.lease_side
                {
                    return Ok(lease);
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

#[tokio::test]
async fn native_directory_expiry_refresh_keeps_the_host_read_generation() -> TestResult {
    // Arrange an actual native Copy and committed Registry grant through Claim.
    let fixture = claim_fixture::ClaimFixture::start().await?;
    let project = ProjectId::generate();
    let (_, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let ProjectClaimReply::Claimed { epoch, .. } = fixture.claim(&project).await? else {
        return Err("native Claim did not publish the writer".into());
    };
    let root = fixture.directory.path().join("nonholder");
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let reader_origin = ActorOrigin {
        installation: "550e8400-e29b-41d4-a716-446655440009"
            .to_owned()
            .try_into()?,
        machine: "native Directory reader".to_owned().try_into()?,
    };
    let enrolled = fixture
        .central
        .additional_machine(
            &reader_origin,
            collaboration_runtime::load_installation_peer(&root)?,
        )
        .await?;
    let reader_identity = MachineIdentity::new(
        UuidIdentity::try_from(reader_origin.installation.as_str().to_owned())?,
        Some(reader_origin.machine.as_str()),
    )?;
    let board =
        open_board_admission(&root, reader_identity, Some(enrolled.session_inputs()?)).await?;
    let session = board.session.as_ref().ok_or("reader session")?;
    let registry = session.projects.registry()?;
    let requests = session.requests.clone();
    let ready = session.welcomed.subscribe();
    let admission = board.admission.clone();
    let running = board.start().ok_or("reader session not started")?;
    wait_welcome(ready).await?;
    let application = collaboration_service::CollaborationApplication::new(
        collaboration_service::ServiceIdentity::new(
            reader_origin.installation.as_str(),
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone()),
    );
    let proof = async {
        let original = directory_lease(&requests, &project).await?;
        let DirectoryLease::Held {
            expires_at,
            holder,
            epoch: observed_epoch,
            ..
        } = &original.lease
        else {
            return Err("native Directory omitted the committed grant".into());
        };
        assert_eq!(*observed_epoch, epoch);
        assert_eq!(*holder, origin()?.installation);
        let expires_at = *expires_at;
        fixture
            .central
            .runtime_control(
                ObjectAddress::Fleet,
                RuntimeControl::SetTime {
                    now: serde_json::from_value(serde_json::to_value(
                        expires_at - chrono::Duration::milliseconds(1),
                    )?)?,
                },
            )
            .await?;
        let held = directory_lease(&requests, &project).await?;
        assert!(matches!(
            held.lease,
            DirectoryLease::Held {
                state: DirectoryLeaseState::Held,
                ..
            }
        ));
        let request = ProjectViewRequest {
            project_id: project.clone(),
            include_removed: false,
        };
        let before = timeout(BOUND, application.board().view_work(request.clone())).await??;
        assert_eq!(before.source, CopySource::ReadCopy {});
        assert_eq!(
            before.as_of,
            expected
                .envelope()
                .position
                .project_cursor()
                .ok_or("source project feed position")?
        );
        let observation = before
            .writer_observation
            .ok_or("native writer observation")?;
        assert_eq!(observation.lease_version, held.lease_version);
        assert_eq!(observation.as_of, held.as_of);
        let handle = registry
            .read_project(&project)
            .await?
            .ok_or("pinned central generation")?;
        let mut pinned = handle.snapshot().await?;
        drop(handle);

        // Change only the real Fleet read clock. No renewal, takeover or lease write.
        fixture
            .central
            .runtime_control(
                ObjectAddress::Fleet,
                RuntimeControl::SetTime {
                    now: serde_json::from_value(serde_json::to_value(expires_at)?)?,
                },
            )
            .await?;
        let expired = directory_lease(&requests, &project).await?;
        assert!(matches!(
            expired.lease,
            DirectoryLease::Held {
                state: DirectoryLeaseState::Expired,
                ..
            }
        ));
        assert_eq!(expired.lease_version, held.lease_version);
        assert_eq!(expired.as_of, held.as_of);
        let after = timeout(BOUND, application.board().view_work(request))
            .await?
            .map_err(|error| {
                format!("A9 rejected native expiry at unchanged version/as_of: {error:?}")
            })?;
        assert_eq!(after.source, CopySource::ReadCopy {});
        assert_eq!(after.as_of, before.as_of);
        assert_eq!(after.replicated_through, before.replicated_through);
        assert_eq!(
            after.current_writer,
            CurrentWriter::Held {
                epoch: epoch.value().try_into()?
            }
        );
        assert_eq!(after.writer_observation, Some(observation));
        assert_eq!(
            pinned.record(&project.clone().into()).await?,
            Some(expected)
        );
        pinned.finish().await?;
        assert!(
            registry.project_ids().await?.is_empty(),
            "reader constructed a local writer"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(application);
    drop(requests);
    drop(registry);
    let stopped = running.shutdown().await;
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| "reader admission retained")?
        .close()
        .await;
    let disconnected = enrolled.shutdown().await;
    let holder_closed = fixture.shutdown().await;
    proof?;
    stopped?;
    closed?;
    disconnected?;
    holder_closed?;
    Ok(())
}
