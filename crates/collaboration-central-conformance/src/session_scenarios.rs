use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult, HolderSession,
    holder_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) struct SessionFixture<'a> {
    pub(crate) client: &'a ConformanceClient,
    pub(crate) old: HolderFixture,
    pub(crate) new: HolderFixture,
    pub(crate) old_socket: HolderSession,
    pub(crate) new_socket: HolderSession,
    pub(crate) project: ProjectId,
}
impl<'a> SessionFixture<'a> {
    pub(crate) async fn create(client: &'a ConformanceClient) -> ConformanceResult<Self> {
        client.set_time(time()?).await?;
        let old = HolderFixture::enroll(client, "old").await?;
        let new = HolderFixture::enroll(client, "new").await?;
        let (old_socket, _) = old.connect(client).await?;
        let (new_socket, _) = new.connect(client).await?;
        let project = project()?;
        let mut fixture = Self {
            client,
            old,
            new,
            old_socket,
            new_socket,
            project,
        };
        fixture.seed_project().await?;
        Ok(fixture)
    }
    pub(crate) async fn seed_project(&mut self) -> ConformanceResult<()> {
        // The Fleet acceptance boundary is fixed here until M3.5 supplies real
        // admission; the registry, copy, session and SQL paths are all real.
        check(
            matches!(self.client.call(ObjectAddress::Registry,WireCall::Registry(RegistryCall::ReserveFirstEpoch{project:self.project.clone(),source:ReservationSource::FleetAcceptance{installation:self.old.installation.clone()}})).await?,WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease{epoch,..}))if epoch==crate::holder_fixtures::epoch(1)?),
            "first lease",
        )?;
        equal(
            self.old_socket
                .request(SessionRequest::Piece(piece(
                    &self.project,
                    position(1, 0)?,
                    position(1, 1)?,
                    vec![schema_transaction()],
                )?))
                .await?,
            SessionAnswer::Acked {
                through: position(1, 1)?,
            },
        )?;
        Ok(())
    }
    pub(crate) async fn preview(&self) -> ConformanceResult<TakeoverPreview> {
        match self
            .client
            .admin(AdminRequest::TakeoverPreview {
                project: self.project.clone(),
            })
            .await?
        {
            AdminResult::Preview(preview) => Ok(preview),
            result => Err(format!("preview result {result:?}").into()),
        }
    }
    pub(crate) fn takeover(&self, preview: TakeoverPreview) -> ConformanceResult<AdminRequest> {
        Ok(AdminRequest::Takeover {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: self.project.clone(),
            to_installation: self.new.installation.clone(),
            reason: "conformance takeover".into(),
            confirm_token: preview.confirm_token,
        })
    }
    pub(crate) fn renew(
        &self,
        attempt: RequestId,
        epoch_value: u64,
    ) -> ConformanceResult<SessionRequest> {
        Ok(SessionRequest::Renew {
            attempt,
            project: self.project.clone(),
            epoch: epoch(epoch_value)?,
        })
    }
    pub(crate) async fn copy(&self) -> ConformanceResult<CopyStatus> {
        match self
            .client
            .call(
                ObjectAddress::Copy(self.project.clone()),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: self.project.clone(),
                }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::CopyStatus(Some(status))) => Ok(status),
            _ => Err("copy status".into()),
        }
    }
    pub(crate) async fn close(self) -> ConformanceResult<()> {
        self.old_socket.close().await?;
        self.new_socket.close().await?;
        Ok(())
    }
}
pub async fn run_session_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    crate::run_call_edge_conformance(client).await?;
    lost_piece_and_resume(client).await?;
    renewals_and_supersession(client).await?;
    transfer_crash_and_preview(client).await?;
    move_refusals(client).await?;
    crate::transfer_scenarios::run_transfer_conformance(client).await?;
    replaced_inflight_and_credentials(client).await?;
    crate::unproven_transfer_rows::run_unproven_transfer_rows(client).await?;
    Ok(())
}
pub(crate) async fn lost_piece_and_resume(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let change = piece(
        &f.project,
        position(1, 1)?,
        position(1, 2)?,
        vec![rows(2, vec![row(1, "accepted once")])],
    )?;
    client
        .control(ControlRequest::DropNextAnswer {
            installation: f.old.installation.clone(),
        })
        .await?;
    f.old_socket
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: SessionRequest::Piece(change.clone()),
        })
        .await?;
    client
        .control(ControlRequest::AwaitDroppedAnswer {
            installation: f.old.installation.clone(),
        })
        .await?;
    // Deliberately drop the connection without reading its answer.
    let (old_reconnected, projects) = f.old.connect(client).await?;
    let expected = position(1, 2)?;
    check(
        projects
            .iter()
            .any(|state| state.project == f.project && state.acked == expected),
        "Welcome reconciles lost piece acknowledgement",
    )?;
    f.old_socket = old_reconnected;
    equal(
        f.old_socket
            .request(SessionRequest::Piece(change.clone()))
            .await?,
        SessionAnswer::Acked {
            through: position(1, 2)?,
        },
    )?;
    for address in [
        ObjectAddress::Copy(f.project.clone()),
        ObjectAddress::Registry,
        ObjectAddress::Connection(f.old.installation.clone()),
    ] {
        client.evict(address).await?;
    }
    let page = f
        .old_socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: f.project.clone(),
            },
            after: None,
        })
        .await?;
    check(
        matches!(page,SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page)))if page.position==position(1,2)?&&page.rows==vec![vec![SqlValue::Integer(1),SqlValue::Text("accepted once".into())]]),
        "read-only snapshot after actors lose memory",
    )?;
    let (socket, projects) = f.old.connect(client).await?;
    f.old_socket = socket;
    check(
        projects
            .iter()
            .any(|state| state.project == f.project && state.acked.commit_id.value() == 2),
        "session resumes after copy/registry/connection eviction",
    )?;
    equal(
        f.old_socket.request(SessionRequest::Piece(change)).await?,
        SessionAnswer::Acked {
            through: position(1, 2)?,
        },
    )?;
    println!(
        "SESSION lost piece answer, Welcome reconciliation, replay without duplicate, actors evicted, consistent read: passed"
    );
    f.close().await
}
async fn renewals_and_supersession(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let r1 = RequestId::new(uuid::Uuid::now_v7().to_string())?;
    let request = f.renew(r1, 1)?;
    client
        .control(ControlRequest::DropNextAnswer {
            installation: f.old.installation.clone(),
        })
        .await?;
    f.old_socket
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: request.clone(),
        })
        .await?;
    client
        .control(ControlRequest::AwaitDroppedAnswer {
            installation: f.old.installation.clone(),
        })
        .await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::minutes(15)))
        .await?;
    let (socket, _) = f.old.connect(client).await?;
    f.old_socket = socket;
    let replay = f.old_socket.request(request.clone()).await?;
    check(
        matches!(&replay,SessionAnswer::Renewed{expires_at,replayed:true,current,..} if *expires_at==time()?.0+chrono::TimeDelta::hours(4)&&current.state==LeaseState::Held),
        "R1 replay does not extend expiry",
    )?;
    let r2 = f.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    check(
        matches!(f.old_socket.request(r2).await?,SessionAnswer::Renewed{expires_at,replayed:false,..} if expires_at==time()?.0+chrono::TimeDelta::hours(4)+chrono::TimeDelta::minutes(15)),
        "fresh R2 extends lease",
    )?;
    let takeover = f.takeover(f.preview().await?)?;
    equal(
        client.admin(takeover.clone()).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        },
    )?;
    equal(
        client.admin(takeover).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        },
    )?;
    check(
        matches!(f.old_socket.request(request).await?,SessionAnswer::Renewed{expires_at,replayed:true,current,..} if expires_at==time()?.0+chrono::TimeDelta::hours(4)&&current.epoch==epoch(2)?&&current.holder==f.new.installation),
        "R1 recorded success carries superseded current authority",
    )?;
    let fresh = f.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    equal(
        f.old_socket.request(fresh).await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2)?,
            epoch_ended_at: position(1, 1)?,
        }),
    )?;
    f.close().await?;
    let mut f = SessionFixture::create(client).await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::hours(5)))
        .await?;
    let (socket, projects) = f.old.connect(client).await?;
    f.old_socket = socket;
    check(
        projects
            .iter()
            .any(|state| state.project == f.project && state.lease.state == LeaseState::Expired),
        "registry clock expiry",
    )?;
    let renew = f.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    check(
        matches!(f.old_socket.request(renew).await?,SessionAnswer::Renewed{current,replayed:false,..}if current.state==LeaseState::Held&&current.epoch==epoch(1)?),
        "same epoch renewal restores held lease",
    )?;
    println!(
        "SESSION R1 lost/replaced/replayed, fresh R2, R1 after takeover, stale fresh attempt, expiry and same-epoch renewal: passed"
    );
    f.close().await
}
pub(crate) async fn transfer_crash_and_preview(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let preview = f.preview().await?;
    equal(
        f.old_socket
            .request(SessionRequest::Piece(piece(
                &f.project,
                position(1, 1)?,
                position(1, 2)?,
                vec![rows(2, vec![row(2, "between preview and fence")])],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(1, 2)?,
        },
    )?;
    equal(
        client.admin(f.takeover(preview)?).await?,
        AdminResult::Refused(AdminRefusal::PreviewStale {
            acked: position(1, 2)?,
        }),
    )?;
    equal(f.copy().await?.fence, epoch(1)?)?;
    let takeover = f.takeover(f.preview().await?)?;
    client.control(ControlRequest::InterruptAfterFence).await?;
    check(
        client.admin(takeover.clone()).await.is_err(),
        "interruption must leave outcome unknown",
    )?;
    equal(f.copy().await?.fence, epoch(2)?)?;
    for address in [
        ObjectAddress::Copy(f.project.clone()),
        ObjectAddress::Registry,
    ] {
        client.evict(address).await?;
    }
    equal(
        client.admin(takeover.clone()).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 2)?,
        },
    )?;
    equal(
        client.admin(takeover).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 2)?,
        },
    )?;
    equal(f.copy().await?.fence, epoch(2)?)?;
    equal(
        f.old_socket
            .request(SessionRequest::Piece(piece(
                &f.project,
                position(1, 2)?,
                position(1, 3)?,
                vec![rows(3, vec![row(3, "rejected tail")])],
            )?))
            .await?,
        SessionAnswer::Refused(SessionRefusal::StaleEpoch {
            current_epoch: epoch(2)?,
            epoch_ended_at: position(1, 2)?,
        }),
    )?;
    equal(
        f.new_socket
            .request(SessionRequest::Piece(piece(
                &f.project,
                position(1, 2)?,
                position(2, 1)?,
                vec![rows(1, vec![row(3, "new holder")])],
            )?))
            .await?,
        SessionAnswer::Acked {
            through: position(2, 1)?,
        },
    )?;
    println!(
        "SESSION previewStale changes nothing, interrupted committed fence, actor restart, same-transfer repeat grants one epoch, stale old/new holder apply: passed"
    );
    f.close().await
}
async fn move_refusals(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
    let request = AdminRequest::Move {
        transfer_id: transfer.clone(),
        project: f.project.clone(),
        to_installation: f.new.installation.clone(),
        reason: "clean move".into(),
    };
    equal(
        client.admin(request.clone()).await?,
        AdminResult::Pending {
            transfer_id: transfer.clone(),
        },
    )?;
    let conflict = AdminRequest::Move {
        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
        project: f.project.clone(),
        to_installation: f.new.installation.clone(),
        reason: "second move".into(),
    };
    equal(
        client.admin(conflict).await?,
        AdminResult::Refused(AdminRefusal::TransferInProgress {
            transfer_id: transfer.clone(),
        }),
    )?;
    let renew = f.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    check(
        matches!(f.old_socket.request(renew).await?,SessionAnswer::Renewed{transfer_in_progress:Some(id),..}if id==transfer),
        "renew recovers missed release notice",
    )?;
    equal(
        f.old_socket
            .request(SessionRequest::Release {
                project: f.project.clone(),
                epoch: epoch(1)?,
                transfer: transfer.clone(),
                outcome: ReleaseOutcome::Final(position(1, 2)?),
            })
            .await?,
        SessionAnswer::Refused(SessionRefusal::PositionGap {
            acked: position(1, 1)?,
        }),
    )?;
    f.old_socket
        .request(SessionRequest::Piece(piece(
            &f.project,
            position(1, 1)?,
            position(1, 2)?,
            vec![rows(2, vec![row(2, "drained")])],
        )?))
        .await?;
    let release = SessionRequest::Release {
        project: f.project.clone(),
        epoch: epoch(1)?,
        transfer: transfer.clone(),
        outcome: ReleaseOutcome::Final(position(1, 2)?),
    };
    equal(
        f.old_socket.request(release.clone()).await?,
        SessionAnswer::Released {
            final_position: position(1, 2)?,
        },
    )?;
    equal(
        f.old_socket.request(release).await?,
        SessionAnswer::Released {
            final_position: position(1, 2)?,
        },
    )?;
    equal(
        client.admin(request).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 2)?,
        },
    )?;
    f.close().await?;
    for state in [MigrationState::Pending, MigrationState::Failed] {
        let mut f = SessionFixture::create(client).await?;
        let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
        client
            .admin(AdminRequest::Move {
                transfer_id: transfer.clone(),
                project: f.project.clone(),
                to_installation: f.new.installation.clone(),
                reason: "migration move".into(),
            })
            .await?;
        check(
            matches!(
                f.old_socket
                    .request(SessionRequest::Release {
                        project: f.project.clone(),
                        epoch: epoch(1)?,
                        transfer,
                        outcome: ReleaseOutcome::Refused(HolderReleaseRefusal::MigrationPending {
                            migration: 2,
                            state
                        })
                    })
                    .await?,
                SessionAnswer::ReleaseRefused {
                    reason: ReleaseRefusal::MigrationPending { .. },
                    ..
                }
            ),
            "holder migration refusal",
        )?;
        equal(f.copy().await?.fence, epoch(1)?)?;
        f.close().await?;
    }
    let f = SessionFixture::create(client).await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::minutes(30)))
        .await?;
    check(
        matches!(
            client
                .admin(AdminRequest::Move {
                    transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                    project: f.project.clone(),
                    to_installation: f.new.installation.clone(),
                    reason: "stale holder".into()
                })
                .await?,
            AdminResult::Refused(AdminRefusal::HolderUnreachable { .. })
        ),
        "thirty-minute holder reachability",
    )?;
    println!(
        "SESSION clean move/gap retry/replayed release, one transfer, missed release notice, migration pending/failed, holderUnreachable: passed"
    );
    f.close().await
}
async fn replaced_inflight_and_credentials(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = SessionFixture::create(client).await?;
    let request = f.renew(RequestId::new(uuid::Uuid::now_v7().to_string())?, 1)?;
    client
        .control(ControlRequest::HoldNextDispatch {
            installation: f.old.installation.clone(),
        })
        .await?;
    f.old_socket
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: request.clone(),
        })
        .await?;
    let SessionRequest::Renew { attempt, .. } = &request else {
        return Err("renewal request".into());
    };
    // The hold names the call it held, so this is the renewal, not another dispatch.
    check(
        matches!(client.control(ControlRequest::AwaitHeldDispatch {installation:f.old.installation.clone()}).await?,
            ControlResult::HeldDispatch(call) if matches!(call.as_ref(),
                WireCall::Registry(RegistryCall::Renew { attempt: held, installation, .. })
                    if held == attempt && installation == &f.old.installation)),
        "held dispatch is this connection's renewal",
    )?;
    let (mut new_socket, _) = f.old.connect(client).await?;
    equal(
        f.old_socket.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::ConnectionReplaced,
        },
    )?;
    client
        .control(ControlRequest::ResumeDispatch {
            installation: f.old.installation.clone(),
        })
        .await?;
    check(
        matches!(
            new_socket.request(request).await?,
            SessionAnswer::Renewed { .. }
        ),
        "inflight request safely re-sent on replacement",
    )?;
    f.old_socket = new_socket;
    equal(
        client
            .admin(AdminRequest::RevokeMachine {
                installation: f.old.installation.clone(),
            })
            .await?,
        AdminResult::MachineRevoked,
    )?;
    equal(
        f.old_socket.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::CredentialRejected(CredentialRejection::Revoked),
        },
    )?;
    check(
        client.connect(&f.old.token).await.is_err(),
        "revoked upgrade rejected",
    )?;
    f.new_socket.close().await?;
    let f = SessionFixture::create(client).await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::days(31)))
        .await?;
    let mut expired = f.old_socket;
    expired
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: SessionRequest::Renew {
                attempt: RequestId::new(uuid::Uuid::now_v7().to_string())?,
                project: f.project,
                epoch: epoch(1)?,
            },
        })
        .await?;
    equal(
        expired.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::CredentialRejected(CredentialRejection::Expired),
        },
    )?;
    check(
        client.connect(&f.old.token).await.is_err(),
        "expired upgrade rejected",
    )?;
    check(
        client
            .connect("unknown-machine-or-agent-credential")
            .await
            .is_err(),
        "unknown/non-enrolled agent token rejected",
    )?;
    check(
        client.connect_owner().await.is_err(),
        "admin cannot open machine session",
    )?;
    // A valid machine credential cannot enter the admin endpoint.
    let response = client
        .client
        .post(format!(
            "{}/admin/v1/projects/{}/takeover/preview",
            client.base_url,
            project()?.as_str()
        ))
        .bearer_auth(&f.new.token)
        .header(
            "Cf-Access-Jwt-Assertion",
            crate::machine_assertion(&f.new.token)?,
        )
        .json(&AdminRequest::TakeoverPreview {
            project: project()?,
        })
        .send()
        .await?;
    equal(response.status(), reqwest::StatusCode::UNAUTHORIZED)?;
    equal(
        response.json::<AccessRefusal>().await?,
        AccessRefusal::WrongAudience,
    )?;
    f.new_socket.close().await?;
    println!(
        "SESSION delayed inflight dispatch/replacement/resend, revoked and expired mid-session/upgrade, unknown/agent/admin rejection, adminRequired: passed"
    );
    Ok(())
}
