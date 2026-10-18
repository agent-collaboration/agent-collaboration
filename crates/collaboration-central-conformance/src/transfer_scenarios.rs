use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult, holder_fixtures::*,
    session_scenarios::SessionFixture,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) async fn run_transfer_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    crate::ordered_transfer_rows::ordered_fence_boundaries(client).await?;
    let mut f = SessionFixture::create(client).await?;
    let unsupported = f.new.connect_supported(client, 0).await?;
    f.new_socket = unsupported.0;
    check(
        matches!(
            client.admin(f.takeover(f.preview().await?)?).await?,
            AdminResult::Refused(AdminRefusal::SchemaTooNew {
                supported: Some(0),
                ..
            })
        ),
        "unsupported target cannot fence",
    )?;
    equal(f.copy().await?.fence, epoch(1)?)?;
    let unknown = AdminRequest::Takeover {
        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
        project: f.project.clone(),
        to_installation: InstallationId::new(uuid::Uuid::now_v7().to_string())?,
        reason: "unknown support".into(),
        confirm_token: f.preview().await?.confirm_token,
    };
    check(
        matches!(
            client.admin(unknown).await?,
            AdminResult::Refused(AdminRefusal::SchemaTooNew {
                supported: None,
                ..
            })
        ),
        "unknown schema support fails closed",
    )?;
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    let moving = AdminRequest::Move {
        transfer_id: transfer.clone(),
        project: f.project.clone(),
        to_installation: f.new.installation.clone(),
        reason: "schema refusal on release".into(),
    };
    client.admin(moving.clone()).await?;
    let released = f
        .old_socket
        .request(SessionRequest::Release {
            project: f.project.clone(),
            epoch: epoch(1)?,
            transfer: transfer.clone(),
            outcome: ReleaseOutcome::Final(position(1, 1)?),
        })
        .await?;
    check(
        matches!(
            released,
            SessionAnswer::ReleaseRefused {
                reason: ReleaseRefusal::SchemaTooNew {
                    supported: Some(0),
                    ..
                },
                ..
            }
        ),
        "release carries typed schema refusal",
    )?;
    check(
        matches!(
            client.admin(moving).await?,
            AdminResult::Refused(AdminRefusal::SchemaTooNew {
                supported: Some(0),
                ..
            })
        ),
        "admin records same refusal",
    )?;
    f.close().await?;
    let f = SessionFixture::create(client).await?;
    // Boundary: preview selection. The copy proves a later fence than the
    // restored registry's floor before the preview reads it, so the takeover
    // selects above it directly and never meets a refused fence.
    equal(
        client
            .call(
                ObjectAddress::Copy(f.project.clone()),
                WireCall::Copy(CopyCall::Fence {
                    transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                    new_epoch: epoch(5)?,
                    expect: FenceExpectation {
                        epoch: epoch(1)?,
                        acked: position(1, 1)?,
                    },
                    target_schema_max: 10,
                    holder: f.old.installation.clone(),
                }),
            )
            .await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(1)?,
            new_epoch: epoch(5)?,
            start: position(1, 1)?,
        })),
    )?;
    equal(
        client.admin(f.takeover(f.preview().await?)?).await?,
        AdminResult::Granted {
            new_epoch: epoch(6)?,
            start: position(1, 1)?,
        },
    )?;
    f.close().await?;
    // Both selection paths run before either is reported, so one regression
    // in the shared retry shows on the takeover and the move together.
    let takeover = takeover_retries_above_out_of_band_fence(client).await;
    let moved = move_retries_above_out_of_band_fence(client).await;
    if takeover.is_err() || moved.is_err() {
        return Err(format!("floor-repair retry: takeover {takeover:?}; move {moved:?}").into());
    }
    held_fence_and_piece_resolve_to_one_complete_prefix(client).await?;
    println!(
        "SESSION schemaTooNew known/unknown and release refusal, registry floor repaired from copy at preview selection, takeover and move retried once above an out-of-band copy fence, held fence and held piece resolve to one complete prefix: passed"
    );
    Ok(())
}

/// Fences the copy ahead of the registry through the copy's own fence call,
/// as a restored registry or a lost transfer would leave it.
async fn fence_copy_out_of_band(f: &SessionFixture<'_>, new_epoch: u64) -> ConformanceResult<()> {
    let call = CopyCall::Fence {
        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
        new_epoch: epoch(new_epoch)?,
        expect: FenceExpectation {
            epoch: epoch(1)?,
            acked: position(1, 1)?,
        },
        target_schema_max: 10,
        holder: f.old.installation.clone(),
    };
    equal(
        f.client
            .call(
                ObjectAddress::Copy(f.project.clone()),
                WireCall::Copy(call.clone()),
            )
            .await?,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(1)?,
            new_epoch: epoch(new_epoch)?,
            start: position(1, 1)?,
        })),
    )
}

/// Boundary: takeover fence. The preview reads copy fence 1, so the takeover
/// selects 2; the copy is fenced to 5 afterwards and refuses 2. Only the
/// floor-repair retry grants 6; without it the takeover is unproven.
async fn takeover_retries_above_out_of_band_fence(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let preview = f.preview().await?;
    equal(f.copy().await?.fence, epoch(1)?)?;
    fence_copy_out_of_band(&f, 5).await?;
    let stale = piece(
        &f.project,
        position(1, 1)?,
        position(1, 2)?,
        vec![rows(2, vec![row(1, "must stay stale")])],
    )?;
    equal(
        f.old_socket
            .request(SessionRequest::Piece(stale.clone()))
            .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(5)?,
            epoch_ended_at: position(1, 1)?,
        }),
    )?;
    let takeover = f.takeover(preview)?;
    equal(
        client.admin(takeover.clone()).await?,
        AdminResult::Granted {
            new_epoch: epoch(6)?,
            start: position(1, 1)?,
        },
    )?;
    let copy = f.copy().await?;
    equal(
        (copy.fence, copy.holder),
        (epoch(6)?, Some(f.new.installation.clone())),
    )?;
    equal(
        client.admin(takeover).await?,
        AdminResult::Granted {
            new_epoch: epoch(6)?,
            start: position(1, 1)?,
        },
    )?;
    equal(
        f.old_socket.request(SessionRequest::Piece(stale)).await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(6)?,
            epoch_ended_at: position(1, 1)?,
        }),
    )?;
    equal(f.copy().await?.fence, epoch(6)?)?;
    equal(
        crate::staging_normal_rows::read_copy(&mut f.old_socket, &f.project).await?,
        (position(1, 1)?, Vec::new()),
    )?;
    println!(
        "STAGING C5: project={} retry grants6/start1,1; identical transfer replays6, old cutoff1,1 unchanged from fence5, old piece stale6",
        f.project.as_str()
    );
    f.close().await
}

/// Boundary: move release. A move selects from the lease epoch when the
/// holder releases, so the copy fenced to 5 after the move began refuses 2.
/// Only the floor-repair retry releases and grants 6.
async fn move_retries_above_out_of_band_fence(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    let moving = AdminRequest::Move {
        transfer_id: transfer.clone(),
        project: f.project.clone(),
        to_installation: f.new.installation.clone(),
        reason: "move above out-of-band fence".into(),
    };
    equal(
        client.admin(moving.clone()).await?,
        AdminResult::Pending {
            transfer_id: transfer.clone(),
        },
    )?;
    fence_copy_out_of_band(&f, 5).await?;
    let released = f
        .old_socket
        .request(SessionRequest::Release {
            project: f.project.clone(),
            epoch: epoch(1)?,
            transfer,
            outcome: ReleaseOutcome::Final(position(1, 1)?),
        })
        .await?;
    // The holder sees any refusal as NotWriter; the admin replay names it.
    equal(
        (released, client.admin(moving).await?),
        (
            SessionAnswer::Released {
                final_position: position(1, 1)?,
            },
            AdminResult::Granted {
                new_epoch: epoch(6)?,
                start: position(1, 1)?,
            },
        ),
    )?;
    let copy = f.copy().await?;
    equal(
        (copy.fence, copy.holder),
        (epoch(6)?, Some(f.new.installation.clone())),
    )?;
    println!(
        "STAGING C6: project={} Pending then final Release grants6/start1,1; copy fence6/target holder",
        f.project.as_str()
    );
    f.close().await
}

/// C3: the takeover's fence and an old-holder piece are both pending at the
/// copy before either is released. The fence is held first; then the piece's
/// own dispatch is held, and the hold must name that piece. Released
/// together, the copy runs one before the other: either the piece commits and
/// the fence is refused previewStale, or the fence commits and the piece is
/// wholly stale. No mix of rows, position and grant is legal.
pub async fn held_fence_and_piece_resolve_to_one_complete_prefix(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let takeover = f.takeover(f.preview().await?)?;
    let change = piece(
        &f.project,
        position(1, 1)?,
        position(1, 2)?,
        vec![rows(2, vec![row(1, "overlap-1"), row(2, "overlap-2")])],
    )?;
    client.control(ControlRequest::HoldNextFence).await?;
    let (granted, overlapped) = tokio::join!(client.admin(takeover), async {
        client.control(ControlRequest::AwaitHeldFence).await?;
        client
            .control(ControlRequest::HoldNextDispatch {
                installation: f.old.installation.clone(),
            })
            .await?;
        let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
        f.old_socket
            .send(&MachineFrame::Request {
                frame_id: frame_id.clone(),
                body: SessionRequest::Piece(change.clone()),
            })
            .await?;
        let held = client
            .control(ControlRequest::AwaitHeldDispatch {
                installation: f.old.installation.clone(),
            })
            .await?;
        check(
            matches!(&held, ControlResult::HeldDispatch(call) if matches!(call.as_ref(),
                WireCall::Copy(CopyCall::ApplyPiece { piece, .. }) if piece.digest == change.digest
                    && piece.project == change.project && piece.through == change.through)),
            "the held dispatch is this piece; another call makes the overlap inconclusive",
        )?;
        println!(
            "STAGING C3 correlated piece dispatch held: project={} frame={} digest={}",
            f.project.as_str(),
            frame_id.as_str(),
            change.digest.as_str()
        );
        client
            .control(ControlRequest::ResumeDispatch {
                installation: f.old.installation.clone(),
            })
            .await?;
        client.control(ControlRequest::ResumeFence).await?;
        println!(
            "STAGING C3 piece and fence holds resumed: project={}",
            f.project.as_str()
        );
        loop {
            match f.old_socket.receive().await? {
                CentralFrame::Answer { frame_id: id, body } if id == frame_id => {
                    return ConformanceResult::Ok(body);
                }
                CentralFrame::Notice { .. } => {}
                frame => return Err(format!("piece answer {frame:?}").into()),
            }
        }
    });
    let (granted, applied) = (granted?, overlapped?);
    let snapshot = f.copy().await?;
    let piece_first = (
        AdminResult::Refused(AdminRefusal::PreviewStale {
            acked: position(1, 2)?,
        }),
        SessionAnswer::Acked {
            through: position(1, 2)?,
        },
    );
    let fence_first = (
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        },
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2)?,
            epoch_ended_at: position(1, 1)?,
        }),
    );
    let outcome = (granted, applied);
    if outcome == piece_first {
        equal(
            (snapshot.fence, snapshot.acked),
            (epoch(1)?, position(1, 2)?),
        )?;
        equal(
            crate::staging_normal_rows::read_copy(&mut f.old_socket, &f.project).await?,
            (
                position(1, 2)?,
                vec![
                    vec![SqlValue::Integer(1), SqlValue::Text("overlap-1".into())],
                    vec![SqlValue::Integer(2), SqlValue::Text("overlap-2".into())],
                ],
            ),
        )?;
        println!("SESSION C3 overlap: piece committed first, fence refused previewStale");
    } else if outcome == fence_first {
        equal(
            (snapshot.fence, snapshot.acked),
            (epoch(2)?, position(1, 1)?),
        )?;
        equal(
            crate::staging_normal_rows::read_copy(&mut f.old_socket, &f.project).await?,
            (position(1, 1)?, Vec::new()),
        )?;
        println!("SESSION C3 overlap: fence committed first, piece wholly stale");
    } else {
        return Err(format!("overlap outcome is neither order: {outcome:?}").into());
    }
    f.close().await
}
