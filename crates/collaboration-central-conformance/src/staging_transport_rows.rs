//! Normal-artifact move/forward and owner-response loss, through public routes only.
use crate::{
    ConformanceClient, ConformanceResult, HolderSession,
    holder_fixtures::*,
    staging_normal_rows::{expected_rows, import_and_claim, read_copy},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;

pub async fn run_staging_transport_rows(client: &ConformanceClient) -> ConformanceResult<()> {
    clean_move(client).await?;
    forward_roundtrip(client).await?;
    owner_response_loss(client).await
}
async fn release_notice(
    socket: &mut HolderSession,
    project: &ProjectId,
    transfer: &TransferId,
) -> ConformanceResult<()> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match socket.receive().await? {
                CentralFrame::Notice {
                    kind:
                        NoticeKind::ReleaseRequested {
                            project: found,
                            transfer: found_transfer,
                            ..
                        },
                    ..
                } if &found == project && &found_transfer == transfer => return Ok(()),
                CentralFrame::Notice { .. } => {}
                frame => return Err(format!("expected release notice, got {frame:?}").into()),
            }
        }
    })
    .await?
}
async fn clean_move(client: &ConformanceClient) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    let (mut old, _) = a.connect(client).await?;
    let (mut new, _) = b.connect(client).await?;
    let project = import_and_claim(client, &mut old).await?;
    let before = read_copy(&mut old, &project).await?;
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    let request = AdminRequest::Move {
        transfer_id: transfer.clone(),
        project: project.clone(),
        to_installation: b.installation.clone(),
        reason: "M3.10 N1 clean move".into(),
    };
    equal(
        client.admin(request.clone()).await?,
        AdminResult::Pending {
            transfer_id: transfer.clone(),
        },
    )?;
    release_notice(&mut old, &project, &transfer).await?;
    equal(
        old.request(SessionRequest::Release {
            project: project.clone(),
            epoch: epoch(2)?,
            transfer,
            outcome: ReleaseOutcome::Final(before.0),
        })
        .await?,
        SessionAnswer::Released {
            final_position: before.0,
        },
    )?;
    let grant = AdminResult::Granted {
        new_epoch: epoch(3)?,
        start: before.0,
    };
    equal(client.admin(request.clone()).await?, grant.clone())?;
    equal(
        old.request(SessionRequest::Piece(piece(
            &project,
            before.0,
            position(2, 1)?,
            vec![rows(1, vec![row(1, "stale-left"), row(9, "stale-right")])],
        )?))
        .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(3)?,
            epoch_ended_at: before.0,
        }),
    )?;
    equal(read_copy(&mut old, &project).await?, before.clone())?;
    equal(
        new.request(SessionRequest::Piece(piece(
            &project,
            before.0,
            position(3, 1)?,
            vec![rows(1, vec![row(1, "moved-1"), row(4, "moved-4")])],
        )?))
        .await?,
        SessionAnswer::Acked {
            through: position(3, 1)?,
        },
    )?;
    equal(
        read_copy(&mut new, &project).await?,
        (
            position(3, 1)?,
            expected_rows(&[
                (1, "moved-1"),
                (2, "imported-2"),
                (3, "imported-3"),
                (4, "moved-4"),
            ]),
        ),
    )?;
    equal(client.admin(request).await?, grant)?;
    println!(
        "STAGING N1 project={}: Pending, exact release, grant3/start {:?}, whole stale refusal/no change, new holder ack(3,1), identical move replay",
        project.as_str(),
        before.0
    );
    old.close().await?;
    new.close().await
}
async fn forward_roundtrip(client: &ConformanceClient) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    let (mut holder, _) = a.connect(client).await?;
    let (mut origin, _) = b.connect(client).await?;
    let project = import_and_claim(client, &mut holder).await?;
    let before = read_copy(&mut holder, &project).await?;
    let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
    let post = ForwardedPost {
        kind: ForwardedPostKind::ThreadPost,
        body: BoardPayload::new(
            serde_json::json!({"fixture":"M3.10 native forward","requestId":frame_id.as_str()}),
        ),
    };
    origin
        .send(&MachineFrame::Request {
            frame_id: frame_id.clone(),
            body: SessionRequest::Forward {
                project: project.clone(),
                post: post.clone(),
            },
        })
        .await?;
    let expected_label = client
        .staging_access()
        .and_then(|access| access.for_fixture("new"))
        .map_or("new", |installation| installation.label.as_str());
    let forward_id = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match holder.receive().await? {
                CentralFrame::Notice { .. } => {}
                CentralFrame::Forwarded {
                    forward_id,
                    origin,
                    origin_label,
                    project: found,
                    post: found_post,
                } => {
                    equal(origin, b.installation.clone())?;
                    equal(origin_label.as_str(), expected_label)?;
                    equal(found, project.clone())?;
                    equal(found_post, post)?;
                    return ConformanceResult::Ok(forward_id);
                }
                frame => return Err(format!("expected forwarded post, got {frame:?}").into()),
            }
        }
    })
    .await??;
    let reply = BoardPayload::new(
        serde_json::json!({"fixture":"M4-holder-admission-stand-in","reply":"native transport response"}),
    );
    holder
        .send(&MachineFrame::ForwardAnswer {
            forward_id,
            reply: reply.clone(),
        })
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match origin.receive().await? {
                CentralFrame::Notice { .. } => {}
                CentralFrame::Answer {
                    frame_id: found,
                    body,
                } => {
                    equal(found, frame_id)?;
                    equal(
                        body,
                        SessionAnswer::Forwarded(ForwardOutcome::Answered { reply }),
                    )?;
                    return ConformanceResult::Ok(());
                }
                frame => {
                    return Err(format!("expected correlated forward answer, got {frame:?}").into());
                }
            }
        }
    })
    .await??;
    equal(read_copy(&mut holder, &project).await?, before)?;
    println!(
        "STAGING N2 project={}: exact origin/label/post and original-frame reply, unchanged copy; holder is a fixture, not M4 admission or two physical machines",
        project.as_str()
    );
    holder.close().await?;
    origin.close().await
}
async fn owner_response_loss(client: &ConformanceClient) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    let (mut old, _) = a.connect(client).await?;
    let (mut new, _) = b.connect(client).await?;
    let project = import_and_claim(client, &mut old).await?;
    let before = read_copy(&mut old, &project).await?;
    let AdminResult::Preview(preview) = client
        .admin(AdminRequest::TakeoverPreview {
            project: project.clone(),
        })
        .await?
    else {
        return Err("owner preview".into());
    };
    let denied = [
        AdminRequest::Move {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            to_installation: b.installation.clone(),
            reason: "M3.10 denied machine move".into(),
        },
        AdminRequest::Takeover {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            to_installation: b.installation.clone(),
            reason: "M3.10 denied machine takeover".into(),
            confirm_token: preview.confirm_token.clone(),
        },
    ];
    let mut statuses = Vec::new();
    for request in denied {
        let operation = if matches!(request, AdminRequest::Move { .. }) {
            "move"
        } else {
            "takeover"
        };
        let builder = client
            .client
            .post(format!(
                "{}/admin/v1/projects/{}/{operation}",
                client.base_url,
                project.as_str()
            ))
            .bearer_auth(&a.token)
            .json(&request);
        let response = client
            .with_machine_access(builder, &a.token)?
            .send()
            .await?;
        let status = response.status().as_u16();
        drop(response);
        check(
            matches!(status, 302 | 401 | 403),
            "machine admin operation must be denied at an authorization layer",
        )?;
        statuses.push(status);
        equal(read_copy(&mut old, &project).await?, before.clone())?;
    }
    let old_confirm_token = preview.confirm_token.clone();
    let request = AdminRequest::Takeover {
        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
        project: project.clone(),
        to_installation: b.installation.clone(),
        reason: "M3.10 unread owner response body".into(),
        confirm_token: preview.confirm_token,
    };
    let response = client
        .with_owner_access(
            client
                .client
                .post(format!(
                    "{}/admin/v1/projects/{}/takeover",
                    client.base_url,
                    project.as_str()
                ))
                .json(&request),
        )?
        .send()
        .await?;
    check(
        response.status().is_success(),
        "owner takeover headers must succeed",
    )?;
    drop(response); // The application never consumes the original grant body.
    let grant = AdminResult::Granted {
        new_epoch: epoch(3)?,
        start: before.0,
    };
    equal(client.admin(request.clone()).await?, grant.clone())?;
    equal(client.admin(request.clone()).await?, grant)?;
    equal(
        client
            .admin(AdminRequest::Takeover {
                transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                project: project.clone(),
                to_installation: a.installation.clone(),
                reason: "M3.10 old preview after holder change".into(),
                confirm_token: old_confirm_token,
            })
            .await?,
        AdminResult::Refused(AdminRefusal::PreviewStale { acked: before.0 }),
    )?;
    println!(
        "STAGING T1 old-holder/old-epoch preview refused after completed holder change; no additional epoch"
    );
    let mut conflicting = request;
    if let AdminRequest::Takeover { reason, .. } = &mut conflicting {
        reason.push_str(" changed");
    }
    equal(
        client.admin(conflicting).await?,
        AdminResult::Refused(AdminRefusal::ConflictingRequest),
    )?;
    equal(
        new.request(SessionRequest::Piece(piece(
            &project,
            before.0,
            position(3, 1)?,
            vec![rows(1, vec![row(4, "reply-loss-4")])],
        )?))
        .await?,
        SessionAnswer::Acked {
            through: position(3, 1)?,
        },
    )?;
    equal(
        old.request(SessionRequest::Piece(piece(
            &project,
            before.0,
            position(2, 1)?,
            vec![rows(1, vec![row(9, "stale")])],
        )?))
        .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(3)?,
            epoch_ended_at: before.0,
        }),
    )?;
    equal(
        read_copy(&mut new, &project).await?,
        (
            position(3, 1)?,
            expected_rows(&[
                (1, "imported-1"),
                (2, "imported-2"),
                (3, "imported-3"),
                (4, "reply-loss-4"),
            ]),
        ),
    )?;
    println!(
        "STAGING A8/T3 project={}: machine move/takeover statuses={statuses:?}, exact copy unchanged; owner body unread, same grant3/start replayed, changed body conflicts, epoch3 write proves no hidden epoch4",
        project.as_str()
    );
    old.close().await?;
    new.close().await
}
