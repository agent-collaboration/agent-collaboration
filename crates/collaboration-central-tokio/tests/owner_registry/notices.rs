//! Distinct public holder transitions name the prior epoch and notify a re-grant.
use super::*;
use board_sync_protocol::{BoardNotice, NoticeRead, PositionedBoardNotice};

#[tokio::test]
async fn holder_changes_notify_the_previous_holder_and_regrant_without_renewal_duplicates()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof = async {
        let database = fixture.database;
        let first = positioned(database, 1, true, 1)?;
        await_page(&mut fixture.old, std::slice::from_ref(&first)).await?;

        let outward = confirm(durable_steps::Uuid::now_v7(), fixture.preview().await?);
        let reply = fixture.post("takeover", &outward).await?;
        let reply = fixture.complete("takeover", &outward, reply).await?;
        fixture::granted(&reply, "granted", 2, fixture::position(1, 0)?)?;
        let superseded = positioned(database, 2, false, 1)?;
        await_page(&mut fixture.old, &[first.clone(), superseded.clone()]).await?;
        let target_granted = positioned(database, 1, true, 2)?;
        await_page(&mut fixture.target, std::slice::from_ref(&target_granted)).await?;

        let mut returning = confirm(durable_steps::Uuid::now_v7(), fixture.preview().await?);
        returning["toInstallation"] = json!(SERVICE);
        let reply = fixture.post("takeover", &returning).await?;
        let reply = fixture.complete("takeover", &returning, reply).await?;
        fixture::granted(&reply, "granted", 3, fixture::position(1, 0)?)?;
        let regranted = positioned(database, 3, true, 3)?;
        let old_expected = vec![first, superseded, regranted];
        await_page(&mut fixture.old, &old_expected).await?;
        let target_expected = vec![target_granted, positioned(database, 2, false, 2)?];
        await_page(&mut fixture.target, &target_expected).await?;

        // Same business operation and a genuine renewal leave the machine
        // positions unchanged. Renewal revises expiry, not epoch/holder identity.
        assert_eq!(fixture.post("takeover", &returning).await?, reply);
        let renewal = support::answer(
            &mut fixture.old,
            OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
                attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                database,
                epoch: Epoch::new(3)?,
            })),
        )
        .await?;
        assert!(matches!(
            renewal,
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
                replayed: false,
                ..
            }))
        ));
        assert_eq!(page(&mut fixture.old).await?, old_expected);
        assert_eq!(page(&mut fixture.target).await?, target_expected);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.close().await?;
    proof
}

fn positioned(
    database: DatabaseId,
    position: u64,
    granted: bool,
    epoch: u64,
) -> ConformanceResult<PositionedBoardNotice> {
    let project = ProjectId::try_from(database.to_string())?;
    let epoch = Epoch::new(epoch)?;
    Ok(PositionedBoardNotice {
        position: NoticePosition::new(position),
        kind: if granted {
            BoardNotice::LeaseGranted { project, epoch }
        } else {
            BoardNotice::LeaseSuperseded { project, epoch }
        },
    })
}
async fn await_page(
    socket: &mut WebSocket,
    expected: &[PositionedBoardNotice],
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let actual = page(socket).await?;
            if actual == expected {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if !expected.starts_with(&actual) {
                return Err(format!(
                    "holder transition produced different positioned notices: {actual:?}"
                )
                .into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| format!("holder transition never produced {expected:?}"))?
}
async fn page(socket: &mut WebSocket) -> ConformanceResult<Vec<PositionedBoardNotice>> {
    match support::answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: None,
        }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(
            NoticeRead::Page { notices, .. },
        ))) => Ok(notices),
        other => Err(format!(
            "holder notice read returned {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}
