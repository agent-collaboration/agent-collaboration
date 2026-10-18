use crate::{ConformanceClient, ConformanceResult, ControlRequest, holder_fixtures::*};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) struct CopyFixture<'a> {
    pub(crate) client: &'a ConformanceClient,
    pub(crate) project: ProjectId,
    pub(crate) holder: InstallationId,
}
impl<'a> CopyFixture<'a> {
    pub(crate) async fn create(client: &'a ConformanceClient) -> ConformanceResult<Self> {
        let fixture = Self {
            client,
            project: project()?,
            holder: InstallationId::new(uuid::Uuid::now_v7().to_string())?,
        };
        check(
            matches!(
                client
                    .call(
                        fixture.address(),
                        WireCall::Copy(CopyCall::Initialize {
                            project: fixture.project.clone(),
                            epoch: epoch(1)?,
                            holder: fixture.holder.clone()
                        })
                    )
                    .await?,
                WireResult::Copy(CopyResult::Initialized(_))
            ),
            "initialize",
        )?;
        equal(
            fixture
                .apply(piece(
                    &fixture.project,
                    position(1, 0)?,
                    position(1, 1)?,
                    vec![schema_transaction()],
                )?)
                .await?,
            SessionAnswer::Acked {
                through: position(1, 1)?,
            },
        )?;
        Ok(fixture)
    }
    pub(crate) fn address(&self) -> ObjectAddress {
        ObjectAddress::Copy(self.project.clone())
    }
    pub(crate) async fn apply(&self, piece: Piece) -> ConformanceResult<SessionAnswer> {
        match self
            .client
            .call(
                self.address(),
                WireCall::Copy(CopyCall::ApplyPiece {
                    piece,
                    sender: self.holder.clone(),
                }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::ApplyPiece(answer)) => Ok(answer),
            _ => Err("apply Return kind".into()),
        }
    }
    pub(crate) async fn state(&self) -> ConformanceResult<CopyStatus> {
        match self
            .client
            .call(
                self.address(),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: self.project.clone(),
                }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::CopyStatus(Some(state))) => Ok(state),
            _ => Err("status Return kind".into()),
        }
    }
    pub(crate) async fn snapshot(&self) -> ConformanceResult<SnapshotPage> {
        match self
            .client
            .call(
                self.address(),
                WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
                ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
            ))) => Ok(*page),
            _ => Err("snapshot Return kind".into()),
        }
    }
}
/// Shared engine scenarios: every assertion is an observable core outcome or
/// a separate snapshot/status request against committed native/DO storage.
pub async fn run_copy_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    client.set_time(time()?).await?;
    ordered_and_receipts(client).await?;
    staging_and_missing_parts(client).await?;
    fences_and_refusals(client).await?;
    rollback_and_admission(client).await?;
    return_contract().await?;
    crate::copy_extended_scenarios::run_extended_copy_conformance(client).await?;
    Ok(())
}
async fn ordered_and_receipts(client: &ConformanceClient) -> ConformanceResult<()> {
    let copy = CopyFixture::create(client).await?;
    let first = piece(
        &copy.project,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(3, vec![row(9, "first")])],
    )?;
    equal(
        copy.apply(first.clone()).await?,
        SessionAnswer::Acked {
            through: position(1, 3)?,
        },
    )?;
    let later = piece(
        &copy.project,
        position(1, 3)?,
        position(1, 7)?,
        vec![
            rows(5, vec![row(9, "updated"), row(10, "second")]),
            rows(
                7,
                vec![RowChange::Delete {
                    table: "records".into(),
                    key: vec![SqlValue::Integer(10)],
                }],
            ),
        ],
    )?;
    copy.apply(later).await?;
    client.evict(copy.address()).await?;
    equal(
        copy.apply(first).await?,
        SessionAnswer::Acked {
            through: position(1, 3)?,
        },
    )?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 1)?,
            position(1, 3)?,
            vec![rows(3, vec![row(9, "different")])],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::PieceConflict),
    )?;
    let page = copy.snapshot().await?;
    equal(page.position, position(1, 7)?)?;
    equal(page.table.as_str(), "records")?;
    equal(
        page.rows,
        vec![vec![SqlValue::Integer(9), SqlValue::Text("updated".into())]],
    )?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 8)?,
            position(1, 9)?,
            vec![rows(9, vec![row(11, "gap")])],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::PositionGap {
            acked: position(1, 7)?,
        }),
    )?;
    let result = client
        .call(
            copy.address(),
            WireCall::Copy(CopyCall::SnapshotPage {
                cursor: Some(SnapshotCursor {
                    position: position(1, 3)?,
                    table: "records".into(),
                    after_key: Vec::new(),
                }),
            }),
        )
        .await?;
    check(
        matches!(result,WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Moved{position:p})))) if p==position(1,7)?),
        "snapshot must restart after position moves",
    )?;
    println!(
        "COPY ordered apply, permanent original receipt replay, conflict, gap, read position, moved snapshot: passed"
    );
    Ok(())
}
async fn staging_and_missing_parts(client: &ConformanceClient) -> ConformanceResult<()> {
    let copy = CopyFixture::create(client).await?;
    let fragments = parts(&copy.project, 2)?;
    let first = fragments.first().ok_or("missing fixture part")?.clone();
    let last = fragments.get(1).ok_or("missing fixture part")?.clone();
    let staged = copy.apply(first.clone()).await?;
    check(
        matches!(staged,SessionAnswer::PartStaged{index:0,acked,..}if acked==position(1,1)?),
        "staged ack unchanged",
    )?;
    equal(copy.apply(first.clone()).await?, staged)?;
    let mut changed = first.clone();
    changed.transactions = vec![rows(4, vec![row(0, "changed")])];
    changed.digest = changed.recomputed_digest()?;
    equal(
        copy.apply(changed).await?,
        SessionAnswer::Refused(SessionRefusal::PieceConflict),
    )?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "staged rows invisible",
    )?;
    client.evict(copy.address()).await?;
    equal(
        copy.apply(last.clone()).await?,
        SessionAnswer::Acked {
            through: position(1, 4)?,
        },
    )?;
    client.evict(copy.address()).await?;
    for part in [first, last] {
        equal(
            copy.apply(part).await?,
            SessionAnswer::Acked {
                through: position(1, 4)?,
            },
        )?;
    }
    equal(copy.snapshot().await?.rows.len(), 2)?;
    for cleanup in [false, true] {
        client.set_time(time()?).await?;
        let copy = CopyFixture::create(client).await?;
        let fragments = parts(&copy.project, 3)?;
        copy.apply(fragments.first().ok_or("missing fixture part")?.clone())
            .await?;
        if cleanup {
            client
                .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::days(2)))
                .await?;
        }
        equal(
            copy.apply(fragments.get(2).ok_or("missing fixture part")?.clone())
                .await?,
            SessionAnswer::Refused(SessionRefusal::PositionGap {
                acked: position(1, 1)?,
            }),
        )?;
        check(
            copy.snapshot().await?.rows.is_empty(),
            "short transaction must never apply",
        )?;
    }
    client.set_time(time()?).await?;
    let copy = CopyFixture::create(client).await?;
    let mut fragments = parts(&copy.project, 2)?;
    for fragment in &mut fragments {
        let descriptor = fragment.part.as_mut().ok_or("part")?;
        descriptor.transaction_digest = PieceDigest::try_from("a".repeat(64))?;
        fragment.digest = fragment.recomputed_digest()?;
    }
    copy.apply(fragments.first().ok_or("missing fixture part")?.clone())
        .await?;
    equal(
        copy.apply(fragments.get(1).ok_or("missing fixture part")?.clone())
            .await?,
        SessionAnswer::Refused(SessionRefusal::PositionGap {
            acked: position(1, 1)?,
        }),
    )?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "whole digest mismatch must not apply",
    )?;
    println!(
        "COPY staged repeat/conflict, restart assembly, per-part permanent replay, missing middle, expired cleanup, transaction digest: passed"
    );
    Ok(())
}
async fn fences_and_refusals(client: &ConformanceClient) -> ConformanceResult<()> {
    let copy = CopyFixture::create(client).await?;
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    let next_holder = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
    let fence = WireCall::Copy(CopyCall::Fence {
        transfer_id: transfer,
        new_epoch: epoch(2)?,
        expect: FenceExpectation {
            epoch: epoch(1)?,
            acked: position(1, 1)?,
        },
        target_schema_max: 10,
        holder: next_holder.clone(),
    });
    equal(
        client.call(copy.address(), fence.clone()).await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(1)?,
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        })),
    )?;
    client.evict(copy.address()).await?;
    equal(
        client.call(copy.address(), fence).await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(1)?,
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        })),
    )?;
    equal(copy.state().await?.holder, Some(next_holder))?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 1)?,
            position(1, 3)?,
            vec![rows(3, vec![row(1, "stale")])],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2)?,
            epoch_ended_at: position(1, 1)?,
        }),
    )?;
    let independent = CopyFixture::create(client).await?;
    equal(
        independent
            .apply(piece(
                &independent.project,
                position(1, 1)?,
                position(1, 3)?,
                vec![rows(3, vec![row(1, "other")])],
            )?)
            .await?,
        SessionAnswer::Acked {
            through: position(1, 3)?,
        },
    )?;
    let copy = CopyFixture::create(client).await?;
    for (new, acked, support, result) in [
        (
            1,
            1,
            10,
            FenceResult::FenceNotAdvanced {
                current_epoch: epoch(1)?,
            },
        ),
        (
            2,
            0,
            10,
            FenceResult::PreviewStale {
                acked: position(1, 1)?,
            },
        ),
    ] {
        equal(
            client
                .call(
                    copy.address(),
                    WireCall::Copy(CopyCall::Fence {
                        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                        new_epoch: epoch(new)?,
                        expect: FenceExpectation {
                            epoch: epoch(1)?,
                            acked: position(1, acked)?,
                        },
                        target_schema_max: support,
                        holder: copy.holder.clone(),
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::Fence(result)),
        )?;
    }
    let unsupported = client
        .call(
            copy.address(),
            WireCall::Copy(CopyCall::Fence {
                transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                new_epoch: epoch(2)?,
                expect: FenceExpectation {
                    epoch: epoch(1)?,
                    acked: position(1, 1)?,
                },
                target_schema_max: 0,
                holder: copy.holder.clone(),
            }),
        )
        .await?;
    check(
        matches!(
            unsupported,
            WireResult::Copy(CopyResult::Fence(FenceResult::SchemaTooNew {
                supported: 0,
                ..
            }))
        ),
        "schema before grant",
    )?;
    println!(
        "COPY fence survives eviction, immutable stale cutoff, other project continues, forward/preview/schema refusal: passed"
    );
    Ok(())
}
async fn rollback_and_admission(client: &ConformanceClient) -> ConformanceResult<()> {
    let copy = CopyFixture::create(client).await?;
    let bad = piece(
        &copy.project,
        position(1, 1)?,
        position(1, 3)?,
        vec![rows(3, vec![row(1, "same"), row(2, "same")])],
    )?;
    check(copy.apply(bad).await.is_err(), "late uniqueness must fail")?;
    equal(copy.state().await?.acked, position(1, 1)?)?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "SQL error must roll back prior row",
    )?;
    client
        .control(ControlRequest::CopyFull {
            address: copy.address(),
            after_writes: 2,
        })
        .await?;
    equal(
        copy.apply(piece(
            &copy.project,
            position(1, 1)?,
            position(1, 3)?,
            vec![rows(3, vec![row(1, "first"), row(2, "full")])],
        )?)
        .await?,
        SessionAnswer::Refused(SessionRefusal::CentralCopyFull),
    )?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "full refusal after explicit rollback",
    )?;
    let unsupported = piece(
        &copy.project,
        position(1, 1)?,
        position(1, 3)?,
        vec![
            rows(2, vec![row(1, "before")]),
            ReplicatedTransaction::SchemaStep {
                commit_id: 3.into(),
                migration_version: 2,
                statements: vec![
                    "CREATE TABLE allowed_before(id INTEGER PRIMARY KEY)".into(),
                    "INSERT INTO _CoPy_state(singleton) VALUES(1)".into(),
                ],
            },
        ],
    )?;
    equal(
        copy.apply(unsupported).await?,
        SessionAnswer::Refused(SessionRefusal::SchemaUnsupported {
            form: SchemaStatementForm::Insert,
        }),
    )?;
    client.evict(copy.address()).await?;
    equal(copy.state().await?.acked, position(1, 1)?)?;
    check(
        copy.snapshot().await?.rows.is_empty(),
        "admission refuses complete piece",
    )?;
    println!(
        "COPY SQL error rollback, injected storage-full rollback/mapping, whole-piece schema admission: passed"
    );
    Ok(())
}
async fn return_contract() -> ConformanceResult<()> {
    let id = ObjectCallId::new(uuid::Uuid::now_v7().to_string())?;
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: id.clone(),
        call: Box::new(WireCall::Copy(CopyCall::CopyStatus {
            project: project()?,
        })),
    };
    let returned = WireEffect::Return {
        call_id: id,
        result: WireResult::Copy(CopyResult::CopyStatus(None)),
    };
    check(
        validate_call_returns(&input, &[]).is_err(),
        "missing Return",
    )?;
    check(
        validate_call_returns(&input, &[returned.clone(), returned.clone()]).is_err(),
        "duplicate Return",
    )?;
    check(
        validate_call_returns(
            &input,
            &[WireEffect::Return {
                call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
                result: WireResult::Copy(CopyResult::CopyStatus(None)),
            }],
        )
        .is_err(),
        "wrong Return identity",
    )?;
    check(
        validate_call_returns(
            &input,
            &[WireEffect::Return {
                call_id: match &returned {
                    WireEffect::Return { call_id, .. } => call_id.clone(),
                    _ => return Err("fixture Return".into()),
                },
                result: WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::TailStored)),
            }],
        )
        .is_err(),
        "wrong Return kind",
    )?;
    validate_call_returns(&input, &[returned])?;
    println!("COPY completed-invocation missing/duplicate/mismatched Return guard: passed");
    Ok(())
}
