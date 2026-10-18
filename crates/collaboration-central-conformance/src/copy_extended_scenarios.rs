use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, StorageCorruption,
    copy_scenarios::CopyFixture, holder_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) async fn run_extended_copy_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let project = project()?;
    let sender = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
    equal(
        client
            .call(
                ObjectAddress::Copy(project.clone()),
                WireCall::Copy(CopyCall::ApplyPiece {
                    piece: piece(
                        &project,
                        position(1, 0)?,
                        position(1, 1)?,
                        vec![schema_transaction()],
                    )?,
                    sender,
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::NotInitialized,
        ))),
    )?;
    let copy = CopyFixture::create(client).await?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 1)?,
            position(2, 3)?,
            vec![rows(3, vec![row(1, "future")])],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::NotWriter {
            current: Some(epoch(1)?),
        }),
    )?;
    let mut mismatch = piece(
        &copy.project,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(3, vec![row(1, "mismatch")])],
    )?;
    mismatch.digest = PieceDigest::try_from("a".repeat(64))?;
    equal(
        copy.apply(mismatch).await?,
        SessionAnswer::Refused(SessionRefusal::PieceConflict),
    )?;
    client
        .control(ControlRequest::Corrupt {
            address: copy.address(),
            mutation: StorageCorruption::RemovePieceReceipts,
        })
        .await?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 0)?,
            position(1, 1)?,
            vec![schema_transaction()],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::PositionGap {
            acked: position(1, 1)?,
        }),
    )?;
    client
        .control(ControlRequest::Corrupt {
            address: copy.address(),
            mutation: StorageCorruption::ArchiveCopy,
        })
        .await?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 1)?,
            position(1, 3)?,
            vec![rows(3, vec![row(1, "archived")])],
        )?)
        .await?,
        SessionAnswer::Acked {
            through: position(1, 3)?,
        },
    )?;
    client
        .control(ControlRequest::Corrupt {
            address: copy.address(),
            mutation: StorageCorruption::InvalidateCopyFence,
        })
        .await?;
    check(
        copy.state().await.is_err(),
        "corrupt state fails closed through real stored-row decoding",
    )?;
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(&copy.project, 3)?;
    copy.apply(fragments.first().ok_or("first")?.clone())
        .await?;
    let incompatible = parts(&copy.project, 2)?;
    equal(
        copy.apply(incompatible.last().ok_or("last")?.clone())
            .await?,
        SessionAnswer::Refused(SessionRefusal::PositionGap {
            acked: position(1, 1)?,
        }),
    )?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "inconsistent assembly invisible",
    )?;
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(&copy.project, 2)?;
    for fragment in &fragments {
        copy.apply(fragment.clone()).await?;
    }
    copy.apply(piece(
        &copy.project,
        position(1, 4)?,
        position(1, 7)?,
        vec![rows(7, vec![row(9, "later")])],
    )?)
    .await?;
    client.evict(copy.address()).await?;
    for part in &fragments {
        equal(
            copy.apply(part.clone()).await?,
            SessionAnswer::Acked {
                through: position(1, 4)?,
            },
        )?;
        let mut changed = part.clone();
        changed.transactions = vec![rows(4, vec![row(99, "changed")])];
        changed.digest = changed.recomputed_digest()?;
        equal(
            copy.apply(changed).await?,
            SessionAnswer::Refused(SessionRefusal::PieceConflict),
        )?;
    }
    let copy = CopyFixture::create(client).await?;
    copy.apply(piece(
        &copy.project,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(
            3,
            (0..300).map(|id| row(id, &format!("row-{id}"))).collect(),
        )],
    )?)
    .await?;
    let page = copy.snapshot().await?;
    equal(page.rows.len(), 256)?;
    let cursor = page.next.ok_or("continuation")?;
    client.evict(copy.address()).await?;
    let next = client
        .call(
            copy.address(),
            WireCall::Copy(CopyCall::SnapshotPage {
                cursor: Some(cursor.clone()),
            }),
        )
        .await?;
    check(
        matches!(next,WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page)))))if page.rows.len()==44&&page.position==position(1,3)?&&page.next.is_none()),
        "lossless snapshot continuation after actor restart",
    )?;
    copy.apply(piece(
        &copy.project,
        position(1, 3)?,
        position(1, 4)?,
        vec![rows(4, vec![row(400, "new")])],
    )?)
    .await?;
    equal(
        client
            .call(
                copy.address(),
                WireCall::Copy(CopyCall::SnapshotPage {
                    cursor: Some(cursor),
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Moved {
                position: position(1, 4)?,
            }),
        ))),
    )?;
    println!(
        "COPY notInitialized/future epoch/digest/archived, missing receipt never blind ack, corrupt stored state, inconsistent parts, per-part changed digest after later commit, paged snapshot continuation: passed"
    );
    Ok(())
}
