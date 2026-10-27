//! A known remote project is a non-writer scope even without a local project file.
use super::*;
use message_board::public_replies::RequestResult as PublicResult;

async fn eligible_task_request(
    holder: &claim_fixture::ClaimFixture,
    project: &ProjectId,
) -> Result<ProjectRequest, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = post_request(project)?;
    grant_posting_role(holder, &request).await?;
    let workstream = WorkstreamId::generate();
    let setup = ProjectRequest {
        request_id: RequestId::generate(),
        operation: BoardOperation::WorkstreamCreate {
            workstream_id: workstream.clone(),
            name: ResourceName::try_from("cached authority fixture".to_owned())?,
            goal: "prove a non-holder task write is refused before admission".to_owned(),
        },
        ..request.clone()
    };
    assert!(matches!(
        holder.admission.submit(&setup).await?.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    request.operation = BoardOperation::TaskCreate {
        task_id: TaskId::generate(),
        workstream_id: workstream,
        title: TaskTitle::try_from("synthetic remote task".to_owned())?,
        body: "this task must never be admitted by the non-holder".to_owned(),
        blocked_by: Vec::new(),
        related_to: Vec::new(),
    };
    Ok(request)
}

async fn assert_cached_task_refusal(complete_read_copy: bool) -> TestResult {
    let holder = claim_fixture::ClaimFixture::start().await?;
    let project = claimed_project(&holder).await?;
    let request = eligible_task_request(&holder, &project).await?;
    let mut sender = Box::pin(SenderFixture::start(&holder)).await?;
    let proof = async {
        let context = sender.admission.runtime_test_context();
        // Welcome refresh is asynchronous. Observe the actual retained directory
        // entry before asking the write boundary to classify this project.
        timeout(BOUND, async {
            loop {
                context.fleet.synchronize().await?;
                if matches!(
                    context.fleet.record(&project.clone().into()).await?,
                    Some(Record::DirectoryEntry(entry)) if entry.id == project
                ) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        assert!(context.registry.existing_project(&project).await?.is_none());
        let expected_epoch = if complete_read_copy {
            // Populate the completed read generation through the real public
            // view path. Its literal epoch comes from the import/claim contract.
            timeout(BOUND, async {
                loop {
                    let view = sender
                        .application
                        .board()
                        .view_work(ProjectViewRequest {
                            project_id: project.clone(),
                            include_removed: false,
                        })
                        .await?;
                    if view.current_writer
                        == (CurrentWriter::Held {
                            epoch: EpochNumber::try_from(2_u64)?,
                        })
                    {
                        assert_eq!(view.source, CopySource::ReadCopy {});
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await??;
            Some(EpochNumber::try_from(2_u64)?)
        } else {
            None
        };
        // The task request must use retained local state after the transport
        // ends; setup alone is allowed to fetch the directory or read copy.
        sender.disconnect().await?;
        let store = holder
            .registry
            .existing_project(&project)
            .await?
            .ok_or("holder store")?;
        let read = store.snapshot().await?;
        let before = read.high_water();
        read.finish().await?;
        let reply = timeout(
            BOUND,
            sender.application.board().task_create(request.clone()),
        )
        .await??;
        assert_eq!(
            reply.result,
            PublicResult::NotWriter {
                current: expected_epoch
            }
        );
        assert!(reply.reader.is_none());

        let unknown = ProjectId::generate();
        assert!(
            context
                .fleet
                .record(&unknown.clone().into())
                .await?
                .is_none()
        );
        let unknown_request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: unknown.clone(),
            ..request.clone()
        };
        let unknown_reply = timeout(
            BOUND,
            sender
                .application
                .board()
                .task_create(unknown_request.clone()),
        )
        .await??;
        assert_eq!(
            unknown_reply.result,
            PublicResult::ScopeUnavailable(ScopeRefusal::ProjectNotFound {
                project: unknown.clone()
            })
        );
        assert!(unknown_reply.reader.is_none());
        let mut read = store.snapshot().await?;
        assert_eq!(
            read.high_water(),
            before,
            "a refused task changed the holder"
        );
        assert!(
            read.record(&request.request_id.clone().into())
                .await?
                .is_none()
        );
        assert!(
            read.record(&unknown_request.request_id.into())
                .await?
                .is_none()
        );
        read.finish().await?;
        assert!(context.fleet.pending_requests().await?.is_empty());
        for absent in [&project, &unknown] {
            assert!(
                !sender
                    .directory
                    .join("projects")
                    .join(absent.as_str())
                    .exists()
            );
        }
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
async fn non_holder_task_write_uses_fleet_membership_without_a_local_copy() -> TestResult {
    Box::pin(assert_cached_task_refusal(false)).await
}

#[tokio::test]
async fn non_holder_task_write_keeps_the_completed_read_copy_epoch() -> TestResult {
    Box::pin(assert_cached_task_refusal(true)).await
}
