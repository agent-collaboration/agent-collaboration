use super::*;
use fixture::{Live, Reader, assert_visible, create, update};
use ingress::ReadPoint;

#[tokio::test]
async fn trigger_during_sync_reads_changes_committed_after_its_first_read() -> TestResult {
    // Hold a real central reply after its read. Coalescing the next trigger
    // into that stale read leaves the literal updated name absent locally.
    let directory = tempfile::tempdir()?;
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    let mut live = Live::start(&directory.path().join("central"), &jwks.url).await?;
    live.commit(create()?).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    let mut reply = reader
        .ingress
        .controls
        .hold(ReadPoint::FeedReplyThrough(2))
        .await;
    let fleet = reader.fleet.clone();
    let syncing = tokio::spawn(async move { fleet.synchronize().await });
    reply.entered().await?;
    live.commit(update("committed after first read")?).await?;
    // Concurrent triggers return promptly; the admission holder must drain them.
    for _ in 0..3 {
        tokio::time::timeout(BOUND, reader.fleet.synchronize()).await??;
    }
    reply.release()?;
    tokio::time::timeout(BOUND, syncing).await???;
    assert_visible(&reader.fleet, &router_root, "committed after first read", 4).await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn trigger_during_resync_requires_another_fresh_read() -> TestResult {
    let directory = tempfile::tempdir()?;
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    let mut live = Live::start(&directory.path().join("central"), &jwks.url).await?;
    live.commit(create()?).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    let mut first = reader
        .ingress
        .controls
        .hold(ReadPoint::FeedReplyThrough(2))
        .await;
    let mut second = reader
        .ingress
        .controls
        .hold(ReadPoint::FeedReplyThrough(4))
        .await;
    let fleet = reader.fleet.clone();
    let syncing = tokio::spawn(async move { fleet.synchronize().await });
    first.entered().await?;
    live.commit(update("first overlapping change")?).await?;
    tokio::time::timeout(BOUND, reader.fleet.synchronize()).await??;
    first.release()?;
    second.entered().await?;
    live.commit(update("change during the resync")?).await?;
    tokio::time::timeout(BOUND, reader.fleet.synchronize()).await??;
    second.release()?;
    tokio::time::timeout(BOUND, syncing).await???;
    assert_visible(&reader.fleet, &router_root, "change during the resync", 6).await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn trigger_during_submit_reads_changes_after_releasing_admission() -> TestResult {
    trigger_during_submission(false).await
}

#[tokio::test]
async fn trigger_during_creation_reads_changes_after_releasing_admission() -> TestResult {
    trigger_during_submission(true).await
}

async fn trigger_during_submission(creation: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    let mut live = Live::start(&directory.path().join("central"), &jwks.url).await?;
    live.commit(create()?).await?;
    let board = BoardId::generate();
    live.commit(fixture::request(FleetOperation::BoardCreate {
        board: board.clone(),
        name: "submission board".to_owned().try_into()?,
        description: "submission overlap proof".to_owned().try_into()?,
        board_kind: BoardKind::Area {},
    })?)
    .await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    let mut reply = reader
        .ingress
        .controls
        .hold(ReadPoint::FeedReplyThrough(6))
        .await;
    let fleet = reader.fleet.clone();
    let submitting = tokio::spawn(async move {
        let origin = ActorOrigin {
            installation: READER_SERVICE.to_owned().try_into()?,
            machine: "cache reader".to_owned().try_into()?,
        };
        let result = if creation {
            let input = ProjectCreateRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: ProjectId::generate(),
                board_id: board.clone(),
                title: "created during overlap".to_owned().try_into()?,
                description: None,
                actor: Identity::Human {
                    human_id: "cache owner".to_owned().try_into()?,
                },
                acting_for: None,
            };
            let mut request = fixture::request(FleetOperation::ProjectDirectoryCreate {
                project: input.project_id.clone(),
                board,
                title: input.title.clone(),
            })?;
            request.request_id = input.request_id.clone();
            fleet.submit_creation(&input, &request, &origin).await?
        } else {
            fleet.submit(&update("submitted change")?, &origin).await?
        };
        assert!(matches!(
            result.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        TestResult::Ok(())
    });
    reply.entered().await?;
    live.commit(update("committed during submission")?).await?;
    tokio::time::timeout(BOUND, reader.fleet.synchronize()).await??;
    reply.release()?;
    tokio::time::timeout(BOUND, submitting).await???;
    assert_visible(
        &reader.fleet,
        &router_root,
        "committed during submission",
        8,
    )
    .await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}
