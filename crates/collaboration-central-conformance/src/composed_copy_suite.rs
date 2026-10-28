//! Surviving shared-copy assertions, using real sessions and physical restart.
use crate::{
    ConformanceClient, ConformanceResult, RuntimeControl,
    composed_client::control,
    composed_copy_fixture::{CopyFixture, equal, parts, piece, position, row, row_key, rows},
};
use fleet_sync_protocol::{
    CommitSequence, CopyUnavailableReason, DatabaseId, Epoch, FenceExpectation, FenceRequest,
    FenceResult, PeerId, PieceDigest, PieceRefusal, ReplicatedTransaction, ReplicationAnswer,
    RowChange, SchemaStatementForm, SchemaSupport, SnapshotCursor, SnapshotRead, SqlValue,
    TransferId,
};
use std::{future::Future, num::NonZeroU64};

pub async fn run_composed_copy_suite<F, Fut>(
    mut client: ConformanceClient,
    mut restart: F,
) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    ordered(&mut client, &mut restart).await?;
    staging(&mut client, &mut restart).await?;
    fences(&mut client, &mut restart).await?;
    rollback(&mut client, &mut restart).await?;
    extended(&mut client, &mut restart).await?;
    println!(
        "COMPOSED_COPY_SUITE ordered_receipts_staging_fences_rollback_paging_and_physical_restarts passed"
    );
    Ok(())
}
async fn ordered<F, Fut>(client: &mut ConformanceClient, restart: &mut F) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    let copy = CopyFixture::create(client).await?;
    let first = piece(
        copy.database,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(3, vec![row(9, "first")?])],
    )?;
    equal(
        copy.apply(client, first.clone()).await?,
        ReplicationAnswer::Applied {
            through: position(1, 3)?,
        },
    )?;
    let delete = RowChange::delete("records", vec![SqlValue::Text(row_key(10))])?;
    copy.apply(
        client,
        piece(
            copy.database,
            position(1, 3)?,
            position(1, 7)?,
            vec![
                rows(5, vec![row(9, "updated")?, row(10, "second")?]),
                rows(7, vec![delete]),
            ],
        )?,
    )
    .await?;
    *client = restart().await?;
    equal(
        copy.apply(client, first).await?,
        ReplicationAnswer::AlreadyApplied {
            through: position(1, 3)?,
        },
    )?;
    equal(
        copy.apply(
            client,
            piece(
                copy.database,
                position(1, 1)?,
                position(1, 3)?,
                vec![rows(3, vec![row(9, "different")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::Refused(PieceRefusal::PieceConflict),
    )?;
    let page = copy.first_page(client).await?;
    equal(page.position, position(1, 7)?)?;
    equal(page.table, "records".to_owned())?;
    equal(
        page.rows,
        vec![vec![
            SqlValue::Text(row_key(9)),
            SqlValue::Text("updated".into()),
        ]],
    )?;
    equal(
        copy.apply(
            client,
            piece(
                copy.database,
                position(1, 8)?,
                position(1, 9)?,
                vec![rows(9, vec![row(11, "gap")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::Refused(PieceRefusal::Gap {
            acked: position(1, 7)?,
        }),
    )?;
    equal(
        copy.page(
            client,
            Some(SnapshotCursor {
                position: position(1, 3)?,
                table: "records".into(),
                after_key: Vec::new(),
            }),
        )
        .await?,
        SnapshotRead::Moved {
            position: position(1, 7)?,
        },
    )?;
    Ok(())
}
async fn staging<F, Fut>(client: &mut ConformanceClient, restart: &mut F) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(copy.database, 2)?;
    let first = fragments.first().ok_or("first part")?.clone();
    let last = fragments.get(1).ok_or("last part")?.clone();
    let staged = copy.apply(client, first.clone()).await?;
    if !matches!(staged,ReplicationAnswer::PartStaged {index:0,acked,..} if acked==position(1,1)?) {
        return Err("staged acknowledgement changed".into());
    }
    equal(copy.apply(client, first.clone()).await?, staged)?;
    let mut changed = first.clone();
    changed.transactions = vec![rows(4, vec![row(0, "changed")?])];
    changed.digest = changed.recomputed_digest()?;
    equal(
        copy.apply(client, changed).await?,
        ReplicationAnswer::Refused(PieceRefusal::PieceConflict),
    )?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    *client = restart().await?;
    equal(
        copy.apply(client, last.clone()).await?,
        ReplicationAnswer::Applied {
            through: position(1, 4)?,
        },
    )?;
    *client = restart().await?;
    for fragment in [first, last] {
        equal(
            copy.apply(client, fragment).await?,
            ReplicationAnswer::AlreadyApplied {
                through: position(1, 4)?,
            },
        )?;
    }
    equal(copy.first_page(client).await?.rows.len(), 2)?;
    for cleanup in [false, true] {
        let copy = CopyFixture::create(client).await?;
        let fragments = parts(copy.database, 3)?;
        copy.apply(client, fragments.first().ok_or("first")?.clone())
            .await?;
        if cleanup {
            control(
                client,
                copy.address()?,
                RuntimeControl::SetTime {
                    now: collaboration_central_core::RegistryTime(
                        chrono::Utc::now() + chrono::TimeDelta::days(2),
                    ),
                },
            )
            .await?;
        }
        equal(
            copy.apply(client, fragments.get(2).ok_or("last")?.clone())
                .await?,
            ReplicationAnswer::Refused(PieceRefusal::Gap {
                acked: position(1, 1)?,
            }),
        )?;
        equal(copy.first_page(client).await?.rows.len(), 0)?;
    }
    let copy = CopyFixture::create(client).await?;
    let mut fragments = parts(copy.database, 2)?;
    for fragment in &mut fragments {
        fragment.part.as_mut().ok_or("part")?.transaction_digest =
            PieceDigest::try_from("a".repeat(64))?;
        fragment.digest = fragment.recomputed_digest()?;
    }
    copy.apply(client, fragments.first().ok_or("first")?.clone())
        .await?;
    equal(
        copy.apply(client, fragments.get(1).ok_or("last")?.clone())
            .await?,
        ReplicationAnswer::Refused(PieceRefusal::Gap {
            acked: position(1, 1)?,
        }),
    )?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    Ok(())
}
async fn fences<F, Fut>(client: &mut ConformanceClient, restart: &mut F) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    let copy = CopyFixture::create(client).await?;
    let holder = PeerId::new(durable_steps::Uuid::now_v7())?;
    let request = FenceRequest {
        database: copy.database,
        transfer_id: TransferId::new(durable_steps::Uuid::now_v7())?,
        new_epoch: Epoch::new(2)?,
        expect: FenceExpectation {
            epoch: Epoch::new(1)?,
            acked: position(1, 1)?,
        },
        target_schema: Some(SchemaSupport { max_version: 10 }),
        holder,
    };
    let expected = FenceResult::Fenced {
        old_epoch: Epoch::new(1)?,
        new_epoch: Epoch::new(2)?,
        start: position(1, 1)?,
    };
    equal(copy.fence(client, request.clone()).await?, expected.clone())?;
    *client = restart().await?;
    equal(copy.fence(client, request).await?, expected)?;
    equal(copy.status(client).await?.holder, Some(holder))?;
    equal(
        copy.apply(
            client,
            piece(
                copy.database,
                position(1, 1)?,
                position(1, 3)?,
                vec![rows(3, vec![row(1, "stale")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {
            newer: Epoch::new(2)?,
            ended_at: position(1, 1)?,
        }),
    )?;
    let independent = CopyFixture::create(client).await?;
    equal(
        independent
            .apply(
                client,
                piece(
                    independent.database,
                    position(1, 1)?,
                    position(1, 3)?,
                    vec![rows(3, vec![row(1, "other")?])],
                )?,
            )
            .await?,
        ReplicationAnswer::Applied {
            through: position(1, 3)?,
        },
    )?;
    let copy = CopyFixture::create(client).await?;
    for (new, acked, result) in [
        (
            1,
            1,
            FenceResult::FenceNotAdvanced {
                current_epoch: Epoch::new(1)?,
            },
        ),
        (
            2,
            0,
            FenceResult::PreviewStale {
                acked: position(1, 1)?,
            },
        ),
    ] {
        equal(
            copy.fence(
                client,
                FenceRequest {
                    database: copy.database,
                    transfer_id: TransferId::new(durable_steps::Uuid::now_v7())?,
                    new_epoch: Epoch::new(new)?,
                    expect: FenceExpectation {
                        epoch: Epoch::new(1)?,
                        acked: position(1, acked)?,
                    },
                    target_schema: Some(SchemaSupport { max_version: 10 }),
                    holder: copy.peer,
                },
            )
            .await?,
            result,
        )?;
    }
    let result = copy
        .fence(
            client,
            FenceRequest {
                database: copy.database,
                transfer_id: TransferId::new(durable_steps::Uuid::now_v7())?,
                new_epoch: Epoch::new(2)?,
                expect: FenceExpectation {
                    epoch: Epoch::new(1)?,
                    acked: position(1, 1)?,
                },
                target_schema: Some(SchemaSupport { max_version: 0 }),
                holder: copy.peer,
            },
        )
        .await?;
    if !matches!(
        result,
        FenceResult::SchemaTooNew {
            supported: Some(0),
            ..
        }
    ) {
        return Err("schema support was not refused before grant".into());
    }
    Ok(())
}
async fn rollback<F, Fut>(client: &mut ConformanceClient, restart: &mut F) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    let copy = CopyFixture::create(client).await?;
    let answer = copy
        .apply(
            client,
            piece(
                copy.database,
                position(1, 1)?,
                position(1, 3)?,
                vec![rows(3, vec![row(1, "same")?, row(2, "same")?])],
            )?,
        )
        .await?;
    if !matches!(answer,ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::Damaged {detail}} if detail.to_ascii_lowercase().contains("unique"))
    {
        return Err("late uniqueness failure absent".into());
    }
    equal(copy.status(client).await?.acked, position(1, 1)?)?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    control(
        client,
        copy.address()?,
        RuntimeControl::CopyFull {
            after_writes: NonZeroU64::new(2).ok_or("positive threshold")?,
        },
    )
    .await?;
    equal(
        copy.apply(
            client,
            piece(
                copy.database,
                position(1, 1)?,
                position(1, 3)?,
                vec![rows(3, vec![row(1, "first")?, row(2, "full")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::CopyUnavailable {
            reason: CopyUnavailableReason::StorageFull,
        },
    )?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    let unsupported = piece(
        copy.database,
        position(1, 1)?,
        position(1, 3)?,
        vec![
            rows(2, vec![row(1, "before")?]),
            ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(3),
                migration_version: 2,
                statements: vec![
                    "CREATE TABLE allowed_before(id TEXT NOT NULL PRIMARY KEY)".into(),
                    "INSERT INTO _FLEET_SYNC_copy(record) VALUES('forged')".into(),
                ],
            },
        ],
    )?;
    equal(
        copy.apply(client, unsupported).await?,
        ReplicationAnswer::Refused(PieceRefusal::SchemaUnsupported {
            form: SchemaStatementForm::Insert,
        }),
    )?;
    *client = restart().await?;
    equal(copy.status(client).await?.acked, position(1, 1)?)?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    Ok(())
}
async fn extended<F, Fut>(client: &mut ConformanceClient, restart: &mut F) -> ConformanceResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ConformanceResult<ConformanceClient>>,
{
    let copy = CopyFixture::create(client).await?;
    let absent = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    equal(
        copy.apply(
            client,
            piece(
                absent,
                position(1, 0)?,
                position(1, 1)?,
                vec![rows(1, vec![row(1, "absent")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::CopyUnavailable {
            reason: CopyUnavailableReason::Uninitialized,
        },
    )?;
    equal(
        copy.apply(
            client,
            piece(
                copy.database,
                position(1, 1)?,
                position(2, 3)?,
                vec![rows(3, vec![row(1, "future")?])],
            )?,
        )
        .await?,
        ReplicationAnswer::Refused(PieceRefusal::NotWriter {
            current: Some(Epoch::new(1)?),
        }),
    )?;
    let mut mismatch = piece(
        copy.database,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(3, vec![row(1, "mismatch")?])],
    )?;
    mismatch.digest = PieceDigest::try_from("a".repeat(64))?;
    equal(
        copy.apply(client, mismatch).await?,
        ReplicationAnswer::Refused(PieceRefusal::PieceConflict),
    )?;
    // Legacy private-table corruption and Return-effect guards are retired;
    // the proof ledger maps their underlying library obligations separately.
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(copy.database, 3)?;
    copy.apply(client, fragments.first().ok_or("first")?.clone())
        .await?;
    equal(
        copy.apply(
            client,
            parts(copy.database, 2)?.last().ok_or("last")?.clone(),
        )
        .await?,
        ReplicationAnswer::Refused(PieceRefusal::Gap {
            acked: position(1, 1)?,
        }),
    )?;
    equal(copy.first_page(client).await?.rows.len(), 0)?;
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(copy.database, 2)?;
    for fragment in &fragments {
        copy.apply(client, fragment.clone()).await?;
    }
    copy.apply(
        client,
        piece(
            copy.database,
            position(1, 4)?,
            position(1, 7)?,
            vec![rows(7, vec![row(9, "later")?])],
        )?,
    )
    .await?;
    *client = restart().await?;
    for fragment in &fragments {
        equal(
            copy.apply(client, fragment.clone()).await?,
            ReplicationAnswer::AlreadyApplied {
                through: position(1, 4)?,
            },
        )?;
        let mut changed = fragment.clone();
        changed.transactions = vec![rows(4, vec![row(99, "changed")?])];
        changed.digest = changed.recomputed_digest()?;
        equal(
            copy.apply(client, changed).await?,
            ReplicationAnswer::Refused(PieceRefusal::PieceConflict),
        )?;
    }
    let copy = CopyFixture::create(client).await?;
    copy.apply(
        client,
        piece(
            copy.database,
            position(1, 1)?,
            position(1, 3)?,
            vec![rows(
                3,
                (0..300)
                    .map(|id| row(id, &format!("row-{id}")))
                    .collect::<Result<Vec<_>, _>>()?,
            )],
        )?,
    )
    .await?;
    let page = copy.first_page(client).await?;
    equal(page.rows.len(), 256)?;
    let cursor = page.next.ok_or("continuation")?;
    *client = restart().await?;
    match copy.page(client, Some(cursor.clone())).await? {
        SnapshotRead::Page(page)
            if page.rows.len() == 44 && page.position == position(1, 3)? && page.next.is_none() => {
        }
        other => return Err(format!("restart continuation {other:?}").into()),
    }
    copy.apply(
        client,
        piece(
            copy.database,
            position(1, 3)?,
            position(1, 4)?,
            vec![rows(4, vec![row(400, "new")?])],
        )?,
    )
    .await?;
    equal(
        copy.page(client, Some(cursor)).await?,
        SnapshotRead::Moved {
            position: position(1, 4)?,
        },
    )?;
    Ok(())
}
