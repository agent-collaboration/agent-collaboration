//! Rows for a staging normal central, the production-guarded artifact with no
//! conformance controls. Projects come from the owner's import and a machine
//! claim; every state is read back through a machine session.
use crate::{
    ConformanceClient, ConformanceResult, HolderSession, holder_fixtures::*, import_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;

pub(crate) async fn import_and_claim(
    client: &ConformanceClient,
    socket: &mut HolderSession,
) -> ConformanceResult<ProjectId> {
    let imported = ImportedProjectFixture::new()?;
    crate::fleet_directory_fixture::import_identity(client, &imported.project).await?;
    imported.upload(client).await?;
    let answer = socket
        .request(claim_request(&imported.project, attempt()?))
        .await?;
    check(
        matches!(&answer, SessionAnswer::Claimed { epoch: claimed, start, .. }
            if *claimed == epoch(2)? && *start == position(1, 3)?),
        "claim grants epoch 2 at the imported head",
    )?;
    Ok(imported.project)
}

fn delete(id: i64) -> RowChange {
    RowChange::Delete {
        table: "records".into(),
        key: vec![SqlValue::Integer(id)],
    }
}

pub(crate) fn expected_rows(rows: &[(i64, &str)]) -> Vec<Vec<SqlValue>> {
    rows.iter()
        .map(|(id, label)| vec![SqlValue::Integer(*id), SqlValue::Text((*label).into())])
        .collect()
}

pub(crate) async fn read_copy(
    socket: &mut HolderSession,
    project: &ProjectId,
) -> ConformanceResult<(ReplicationPosition, Vec<Vec<SqlValue>>)> {
    match socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: project.clone(),
            },
            after: None,
        })
        .await?
    {
        SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page)))
            if page.next.is_none() =>
        {
            Ok((page.position, page.rows))
        }
        answer => Err(format!("project copy read {answer:?}").into()),
    }
}

async fn acked(
    socket: &mut HolderSession,
    piece: Piece,
    through: ReplicationPosition,
) -> ConformanceResult<()> {
    equal(
        socket.request(SessionRequest::Piece(piece)).await?,
        SessionAnswer::Acked { through },
    )
}

/// State carried across the normal central's redeploy.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DeploySocketObservation {
    Unobserved,
    Closed,
    Survived,
}
pub struct NormalRedeployState {
    a: HolderFixture,
    b: HolderFixture,
    project: ProjectId,
    takeover: AdminRequest,
    stale: Piece,
    in_flight: Piece,
    in_flight_frame: FrameId,
    deploy_socket: DeploySocketObservation,
    b_socket: HolderSession,
}

/// F1, F2, T1, T2, T3 (first half), S1 and the setup for S2/F3/T3 across a
/// redeploy. Projects are imported by the owner and claimed by installation
/// A at epoch 2, start (1,3); B is the takeover target.
pub async fn prepare_staging_normal_rows(
    client: &ConformanceClient,
) -> ConformanceResult<NormalRedeployState> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    let (mut a_socket, _) = a.connect(client).await?;
    let (mut b_socket, _) = b.connect(client).await?;
    let project = import_and_claim(client, &mut a_socket).await?;
    let second = import_and_claim(client, &mut a_socket).await?;
    acked(
        &mut a_socket,
        piece(
            &project,
            position(1, 3)?,
            position(2, 1)?,
            vec![rows(1, vec![row(4, "a-4"), row(1, "a-1"), delete(2)])],
        )?,
        position(2, 1)?,
    )
    .await?;
    // T1: an acknowledged piece between preview and confirmation.
    let stale_preview = match client
        .admin(AdminRequest::TakeoverPreview {
            project: project.clone(),
        })
        .await?
    {
        AdminResult::Preview(preview) => preview,
        result => return Err(format!("preview {result:?}").into()),
    };
    acked(
        &mut a_socket,
        piece(
            &project,
            position(2, 1)?,
            position(2, 2)?,
            vec![rows(2, vec![row(5, "a-5")])],
        )?,
        position(2, 2)?,
    )
    .await?;
    // Each confirmation is its own transfer: a reused ID with other content
    // is refused as conflictingRequest.
    let takeover = |confirm_token| -> ConformanceResult<AdminRequest> {
        Ok(AdminRequest::Takeover {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            to_installation: b.installation.clone(),
            reason: "M3.10 staging takeover".into(),
            confirm_token,
        })
    };
    equal(
        client.admin(takeover(stale_preview.confirm_token)?).await?,
        AdminResult::Refused(AdminRefusal::PreviewStale {
            acked: position(2, 2)?,
        }),
    )?;
    acked(
        &mut a_socket,
        piece(
            &project,
            position(2, 2)?,
            position(2, 3)?,
            vec![rows(3, vec![row(6, "a-6")])],
        )?,
        position(2, 3)?,
    )
    .await?;
    println!("STAGING T1 previewStale, fence unchanged (A still applies): passed");
    // T2 and the first half of T3.
    let preview = match client
        .admin(AdminRequest::TakeoverPreview {
            project: project.clone(),
        })
        .await?
    {
        AdminResult::Preview(preview) => preview,
        result => return Err(format!("preview {result:?}").into()),
    };
    let takeover = takeover(preview.confirm_token)?;
    let granted = AdminResult::Granted {
        new_epoch: epoch(3)?,
        start: position(2, 3)?,
    };
    equal(client.admin(takeover.clone()).await?, granted.clone())?;
    equal(client.admin(takeover.clone()).await?, granted)?;
    println!("STAGING T2 takeover granted epoch 3 at the acknowledged position; T3 replay: passed");
    // F1 and F2 on A's one session.
    let stale = piece(
        &project,
        position(2, 3)?,
        position(2, 4)?,
        vec![rows(4, vec![row(7, "stale"), delete(1)])],
    )?;
    let stale_refusal = SessionAnswer::Refused(SessionRefusal::StaleEpoch {
        current_epoch: epoch(3)?,
        epoch_ended_at: position(2, 3)?,
    });
    equal(
        a_socket
            .request(SessionRequest::Piece(stale.clone()))
            .await?,
        stale_refusal,
    )?;
    acked(
        &mut a_socket,
        piece(
            &second,
            position(1, 3)?,
            position(2, 1)?,
            vec![rows(1, vec![row(4, "second-4")])],
        )?,
        position(2, 1)?,
    )
    .await?;
    acked(
        &mut b_socket,
        piece(
            &project,
            position(2, 3)?,
            position(3, 1)?,
            vec![rows(1, vec![row(7, "b-7")])],
        )?,
        position(3, 1)?,
    )
    .await?;
    equal(
        read_copy(&mut b_socket, &project).await?,
        (
            position(3, 1)?,
            expected_rows(&[
                (1, "a-1"),
                (3, "imported-3"),
                (4, "a-4"),
                (5, "a-5"),
                (6, "a-6"),
                (7, "b-7"),
            ]),
        ),
    )?;
    equal(
        read_copy(&mut a_socket, &second).await?,
        (
            position(2, 1)?,
            expected_rows(&[
                (1, "imported-1"),
                (2, "imported-2"),
                (3, "imported-3"),
                (4, "second-4"),
            ]),
        ),
    )?;
    println!(
        "STAGING F1 stale multi-change piece refused with exact cutoff, contents unchanged, new holder applies; F2 other project on the same session acknowledged: passed"
    );
    // S1: a piece whose answer the client never reads.
    let unanswered = piece(
        &second,
        position(2, 1)?,
        position(2, 2)?,
        vec![rows(2, vec![row(5, "second-5"), delete(2)])],
    )?;
    a_socket
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: SessionRequest::Piece(unanswered.clone()),
        })
        .await?;
    a_socket.close().await?;
    let (mut a_socket, projects) = a.connect(client).await?;
    let recovered = projects
        .iter()
        .find(|state| state.project == second)
        .map(|state| state.acked);
    println!("STAGING S1 Welcome after the unread answer: acked {recovered:?}");
    acked(&mut a_socket, unanswered.clone(), position(2, 2)?).await?;
    acked(
        &mut a_socket,
        piece(
            &second,
            position(2, 2)?,
            position(2, 3)?,
            vec![rows(3, vec![row(2, "second-2-again")])],
        )?,
        position(2, 3)?,
    )
    .await?;
    acked(&mut a_socket, unanswered, position(2, 2)?).await?;
    equal(
        read_copy(&mut a_socket, &second).await?,
        (
            position(2, 3)?,
            expected_rows(&[
                (1, "imported-1"),
                (2, "second-2-again"),
                (3, "imported-3"),
                (4, "second-4"),
                (5, "second-5"),
            ]),
        ),
    )?;
    a_socket.close().await?;
    println!(
        "STAGING S1 replay after reconnect and after a later update returns the original ack with one effect: passed"
    );
    // S2: B's piece is in flight when the redeploy starts.
    let in_flight = piece(
        &project,
        position(3, 1)?,
        position(3, 2)?,
        vec![rows(2, vec![row(8, "b-8")])],
    )?;
    let in_flight_frame = FrameId::new(uuid::Uuid::now_v7().to_string())?;
    b_socket
        .send(&MachineFrame::Request {
            frame_id: in_flight_frame.clone(),
            body: SessionRequest::Piece(in_flight.clone()),
        })
        .await?;
    Ok(NormalRedeployState {
        a,
        b,
        project,
        takeover,
        stale,
        in_flight,
        in_flight_frame,
        deploy_socket: DeploySocketObservation::Unobserved,
        b_socket,
    })
}

impl NormalRedeployState {
    /// Observes closure or survival; a deploy is not assumed to close a socket.
    pub async fn await_deploy_close(
        &mut self,
        bound: std::time::Duration,
    ) -> ConformanceResult<bool> {
        let closed = self.b_socket.await_server_close(bound).await?;
        self.deploy_socket = if closed {
            DeploySocketObservation::Closed
        } else {
            DeploySocketObservation::Survived
        };
        Ok(closed)
    }

    async fn read_surviving_socket(&mut self) -> ConformanceResult<()> {
        let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
        let sent = chrono::Utc::now();
        self.b_socket
            .send(&MachineFrame::Request {
                frame_id: frame_id.clone(),
                body: SessionRequest::Read {
                    scope: ReadScope::ProjectCopy {
                        project: self.project.clone(),
                    },
                    after: None,
                },
            })
            .await?;
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    match self.b_socket.receive().await? {
                        CentralFrame::Notice { .. } => {}
                        CentralFrame::Answer {
                            frame_id: found, ..
                        } if found == self.in_flight_frame => {}
                        CentralFrame::Answer {
                            frame_id: found,
                            body:
                                SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(
                                    page,
                                ))),
                        } if found == frame_id && page.next.is_none() => {
                            return ConformanceResult::Ok((page.position, page.rows));
                        }
                        _ => {
                            return Err(
                                "surviving socket must answer its correlated exact read".into()
                            );
                        }
                    }
                }
            })
            .await??;
        let before = (
            position(3, 1)?,
            expected_rows(&[
                (1, "a-1"),
                (3, "imported-3"),
                (4, "a-4"),
                (5, "a-5"),
                (6, "a-6"),
                (7, "b-7"),
            ]),
        );
        let after = (
            position(3, 2)?,
            expected_rows(&[
                (1, "a-1"),
                (3, "imported-3"),
                (4, "a-4"),
                (5, "a-5"),
                (6, "a-6"),
                (7, "b-7"),
                (8, "b-8"),
            ]),
        );
        check(
            read == before || read == after,
            "same surviving socket must return one exact complete in-flight prefix",
        )?;
        println!(
            "STAGING S2 surviving original B socket: project={} frame={} sent={} received={} position={:?}; no Hello/reconnect; version/instance attribution still requires platform logs",
            self.project.as_str(),
            frame_id.as_str(),
            sent,
            chrono::Utc::now(),
            read.0
        );
        Ok(())
    }

    /// S2, F3 and the second half of T3, after a real redeploy.
    pub async fn verify(mut self, client: &ConformanceClient) -> ConformanceResult<()> {
        match self.deploy_socket {
            DeploySocketObservation::Survived => self.read_surviving_socket().await?,
            DeploySocketObservation::Closed => println!(
                "STAGING S2 original socket actually closed; surviving-socket path not applicable"
            ),
            DeploySocketObservation::Unobserved => {
                return Err("deployment socket observation missing".into());
            }
        }
        let (mut b_socket, projects) = self.b.connect(client).await?;
        let recovered = projects
            .iter()
            .find(|state| state.project == self.project)
            .map(|state| (state.acked, state.lease.epoch));
        println!("STAGING S2 Welcome after redeploy: {recovered:?}");
        acked(&mut b_socket, self.in_flight, position(3, 2)?).await?;
        acked(
            &mut b_socket,
            piece(
                &self.project,
                position(3, 2)?,
                position(3, 3)?,
                vec![rows(3, vec![row(9, "b-9")])],
            )?,
            position(3, 3)?,
        )
        .await?;
        equal(
            read_copy(&mut b_socket, &self.project).await?,
            (
                position(3, 3)?,
                expected_rows(&[
                    (1, "a-1"),
                    (3, "imported-3"),
                    (4, "a-4"),
                    (5, "a-5"),
                    (6, "a-6"),
                    (7, "b-7"),
                    (8, "b-8"),
                    (9, "b-9"),
                ]),
            ),
        )?;
        println!(
            "STAGING S2 in-flight piece resolved once after redeploy, next piece applies: passed"
        );
        equal(
            client.admin(self.takeover).await?,
            AdminResult::Granted {
                new_epoch: epoch(3)?,
                start: position(2, 3)?,
            },
        )?;
        println!("STAGING T3 completed takeover replayed after redeploy, no new epoch: passed");
        let (mut a_socket, _) = self.a.connect(client).await?;
        equal(
            a_socket.request(SessionRequest::Piece(self.stale)).await?,
            SessionAnswer::Refused(SessionRefusal::StaleEpoch {
                current_epoch: epoch(3)?,
                epoch_ended_at: position(2, 3)?,
            }),
        )?;
        println!("STAGING F3 old holder still refused with the same cutoff after redeploy: passed");
        a_socket.close().await?;
        b_socket.close().await
    }
}

fn renewal(
    project: &ProjectId,
    attempt: &RequestId,
    epoch_value: u64,
) -> ConformanceResult<SessionRequest> {
    Ok(SessionRequest::Renew {
        attempt: attempt.clone(),
        project: project.clone(),
        epoch: epoch(epoch_value)?,
    })
}

/// S5 on the registry's real clock. R1 is sent without reading its answer;
/// a new connection replaces it before R1 is replayed. The replay after a
/// connection replacement returns the same expiry marked replayed; a fresh R2
/// renews again; after a takeover R1 still replays with the superseded
/// current lease, and a fresh renewal for the old epoch is refused.
pub async fn run_staging_renewal_rows(client: &ConformanceClient) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    // B's Hello records its schema support; a takeover to an unknown
    // installation is refused schemaTooNew.
    let (b_socket, _) = b.connect(client).await?;
    let (mut a_socket, _) = a.connect(client).await?;
    let project = import_and_claim(client, &mut a_socket).await?;
    let r1 = attempt()?;
    // The first R1 answer is never read from this connection.
    a_socket
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: renewal(&project, &r1, 2)?,
        })
        .await?;
    let unobserved = a_socket;
    let (mut a_socket, _) = a.connect(client).await?;
    let SessionAnswer::Renewed {
        expires_at: first_expiry,
        replayed: true,
        ..
    } = a_socket.request(renewal(&project, &r1, 2)?).await?
    else {
        return Err(
            "R1 must already be recorded after its unread answer and connection replacement".into(),
        );
    };
    drop(unobserved);
    check(
        matches!(a_socket.request(renewal(&project, &attempt()?, 2)?).await?,
            SessionAnswer::Renewed { expires_at, replayed: false, .. } if expires_at >= first_expiry),
        "a fresh R2 renews again",
    )?;
    let preview = match client
        .admin(AdminRequest::TakeoverPreview {
            project: project.clone(),
        })
        .await?
    {
        AdminResult::Preview(preview) => preview,
        result => return Err(format!("preview {result:?}").into()),
    };
    check(
        matches!(client.admin(AdminRequest::Takeover {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            to_installation: b.installation.clone(),
            reason: "M3.10 staging renewal supersession".into(),
            confirm_token: preview.confirm_token,
        }).await?, AdminResult::Granted { new_epoch, .. } if new_epoch == epoch(3)?),
        "takeover grants epoch 3",
    )?;
    check(
        matches!(a_socket.request(renewal(&project, &r1, 2)?).await?,
            SessionAnswer::Renewed { expires_at, replayed: true, current, .. }
                if expires_at == first_expiry && current.epoch == epoch(3)? && current.holder == b.installation),
        "R1 replays its recorded success with the superseded current lease",
    )?;
    check(
        matches!(a_socket.request(renewal(&project, &attempt()?, 2)?).await?,
            SessionAnswer::Refused(SessionRefusal::StaleEpoch { current_epoch, .. })
                if current_epoch == epoch(3)?),
        "a fresh renewal for the old epoch is staleEpoch",
    )?;
    a_socket.close().await?;
    b_socket.close().await?;
    println!(
        "STAGING S5 original R1 answer unread; replayed with its expiry after replacement, fresh R2 renews, R1 after takeover shows superseded lease, fresh old-epoch renewal staleEpoch: passed"
    );
    Ok(())
}

/// S6: one socket answers a frame, stays idle for each window, then answers
/// the next frame with no new Hello. The driver prints the times; whether a
/// fresh object instance answered is read from the Worker log's instance
/// lines for that window, never inferred from the wait.
pub async fn run_staging_hibernation_rows(
    client: &ConformanceClient,
    idle_windows: &[std::time::Duration],
) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let (mut socket, _) = a.connect(client).await?;
    let project = import_and_claim(client, &mut socket).await?;
    let expected = (
        position(1, 3)?,
        expected_rows(&[(1, "imported-1"), (2, "imported-2"), (3, "imported-3")]),
    );
    equal(read_copy(&mut socket, &project).await?, expected.clone())?;
    for window in idle_windows {
        println!(
            "STAGING S6 idle window {}s from {}",
            window.as_secs(),
            chrono::Utc::now()
        );
        tokio::time::sleep(*window).await;
        let sent = chrono::Utc::now();
        equal(read_copy(&mut socket, &project).await?, expected.clone())?;
        println!(
            "STAGING S6 same-socket frame sent {sent} answered {} with exact state",
            chrono::Utc::now()
        );
    }
    socket.close().await
}
