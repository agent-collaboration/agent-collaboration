//! Ordered controls for the held overlap case; each boundary uses its own fixture.
use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, holder_fixtures::*,
    session_scenarios::SessionFixture, staging_normal_rows::read_copy,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) async fn ordered_fence_boundaries(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut before = SessionFixture::create(client).await?;
    let takeover = before.takeover(before.preview().await?)?;
    let change = piece(
        &before.project,
        position(1, 1)?,
        position(1, 2)?,
        vec![rows(2, vec![row(1, "before-left"), row(2, "before-right")])],
    )?;
    client.control(ControlRequest::HoldNextFence).await?;
    let (refused, ordered) = tokio::join!(client.admin(takeover), async {
        client.control(ControlRequest::AwaitHeldFence).await?;
        equal(
            before
                .old_socket
                .request(SessionRequest::Piece(change))
                .await?,
            SessionAnswer::Acked {
                through: position(1, 2)?,
            },
        )?;
        equal(
            read_copy(&mut before.old_socket, &before.project).await?,
            (
                position(1, 2)?,
                vec![
                    vec![SqlValue::Integer(1), SqlValue::Text("before-left".into())],
                    vec![SqlValue::Integer(2), SqlValue::Text("before-right".into())],
                ],
            ),
        )?;
        client.control(ControlRequest::ResumeFence).await?;
        ConformanceResult::Ok(())
    });
    ordered?;
    equal(
        refused?,
        AdminResult::Refused(AdminRefusal::PreviewStale {
            acked: position(1, 2)?,
        }),
    )?;
    equal(before.copy().await?.fence, epoch(1)?)?;
    println!(
        "STAGING C4 before: project={} complete piece acked/read before held fence resumed; previewStale, unchanged fence",
        before.project.as_str()
    );
    before.close().await?;

    let mut after = SessionFixture::create(client).await?;
    let takeover = after.takeover(after.preview().await?)?;
    equal(
        client.admin(takeover).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        },
    )?;
    equal(
        after
            .old_socket
            .request(SessionRequest::Piece(piece(
                &after.project,
                position(1, 1)?,
                position(1, 2)?,
                vec![rows(2, vec![row(1, "late-left"), row(2, "late-right")])],
            )?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2)?,
            epoch_ended_at: position(1, 1)?,
        }),
    )?;
    equal(
        read_copy(&mut after.old_socket, &after.project).await?,
        (position(1, 1)?, Vec::new()),
    )?;
    equal(
        after
            .new_socket
            .request(SessionRequest::Piece(piece(
                &after.project,
                position(1, 1)?,
                position(2, 1)?,
                vec![rows(1, vec![row(1, "new-left"), row(2, "new-right")])],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(2, 1)?,
        },
    )?;
    equal(
        read_copy(&mut after.new_socket, &after.project).await?,
        (
            position(2, 1)?,
            vec![
                vec![SqlValue::Integer(1), SqlValue::Text("new-left".into())],
                vec![SqlValue::Integer(2), SqlValue::Text("new-right".into())],
            ],
        ),
    )?;
    println!(
        "STAGING C4 after: project={} grant before late multi-change piece; wholly stale, exact unchanged copy, new holder writes",
        after.project.as_str()
    );
    after.close().await
}
