//! Real Registry/Fleet delivery, local notice reads and physical reconnection.
use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult, HolderSession,
    fleet_directory_fixture::import_identity,
    fleet_request_scenarios::envelope,
    holder_fixtures::*,
    import_fixtures::{ImportedProjectFixture, attempt, claim_request},
    registry_rpc_driver::future_time,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_replication_protocol::*;
use fleet_admission::{DirectoryEntry, FleetRequestResult};
use message_board::RequestDecision;

pub(crate) async fn claimed(
    client: &ConformanceClient,
) -> ConformanceResult<(
    HolderFixture,
    HolderSession,
    DirectoryEntry,
    ProjectId,
    RegistryTime,
)> {
    let at = future_time()?;
    client.set_time(at).await?;
    let fixture = ImportedProjectFixture::new()?;
    let identity = import_identity(client, &fixture.project).await?;
    fixture.upload(client).await?;
    let holder = HolderFixture::enroll(client, "notice holder").await?;
    let (mut socket, _) = holder.connect(client).await?;
    equal(
        socket
            .request(claim_request(&fixture.project, attempt()?))
            .await?,
        SessionAnswer::Claimed {
            epoch: epoch(2)?,
            expires_at: at.0 + chrono::TimeDelta::hours(4),
            start: position(1, 3)?,
        },
    )?;
    Ok((holder, socket, identity, fixture.project, at))
}
pub(crate) async fn retained(
    socket: &mut HolderSession,
    now: RegistryTime,
) -> ConformanceResult<Vec<Notice>> {
    let SessionAnswer::ReadPage(ReadPage::Notices { notices, as_of }) = socket
        .request(SessionRequest::Read {
            scope: ReadScope::Notices,
            after: None,
        })
        .await?
    else {
        return Err("actual local notices page".into());
    };
    equal(as_of, now.0)?;
    for (index, notice) in notices.iter().enumerate() {
        equal(notice.position.value(), index as u64 + 1)?;
    }
    Ok(notices)
}

pub struct NoticeRestartScenario {
    holder: HolderFixture,
    before: Vec<Notice>,
    at: RegistryTime,
}
impl NoticeRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let (holder, mut socket, _, project, at) = claimed(client).await?;
        let before = retained(&mut socket, at).await?;
        equal(
            before.clone(),
            vec![Notice {
                position: NoticePosition::new(1)?,
                kind: NoticeKind::LeaseGranted {
                    project,
                    epoch: epoch(2)?,
                },
            }],
        )?;
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("notice alarm".into());
        };
        check(
            alarms.contains(&(
                ObjectAddress::Connection(holder.installation.clone()),
                RegistryTime(at.0 + chrono::TimeDelta::days(7)),
            )),
            "delivery persisted its exact seven-day platform alarm",
        )?;
        socket.close().await?;
        Ok(Self { holder, before, at })
    }
    pub async fn verify(self, client: &ConformanceClient) -> ConformanceResult<()> {
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("restarted notice alarm".into());
        };
        check(
            alarms.contains(&(
                ObjectAddress::Connection(self.holder.installation.clone()),
                RegistryTime(self.at.0 + chrono::TimeDelta::days(7)),
            )),
            "exact delivery alarm retained across physical restart before clock update",
        )?;
        let reopened = RegistryTime(self.at.0 + chrono::TimeDelta::seconds(1));
        client.set_time(reopened).await?;
        let (mut socket, _) = self.holder.connect(client).await?;
        equal(retained(&mut socket, reopened).await?, self.before)?;
        let expired = RegistryTime(self.at.0 + chrono::TimeDelta::days(8));
        client.set_time(expired).await?;
        check(
            retained(&mut socket, expired).await?.is_empty(),
            "actual session no longer reads seven-day-old notices",
        )?;
        socket.close().await?;
        println!(
            "NOTICES physical restart: real claim→Registry→delivery numbered grant, durable local read, exact pre-clock platform alarm, reconnect and seven-day retention: passed"
        );
        Ok(())
    }
}

pub async fn run_fleet_notice_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let (holder, mut socket, identity, project, at) = claimed(client).await?;
    equal(retained(&mut socket, at).await?.len(), 1)?;
    let SessionAnswer::ReadPage(ReadPage::FleetRecords { position, .. }) = socket
        .request(SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: None,
        })
        .await?
    else {
        return Err("register actual Fleet reader".into());
    };
    equal(position.value(), 3)?;
    let request = crate::archive_runtime::archive_request(&holder, &identity)?;
    let frame = FrameId::new(uuid::Uuid::now_v7().to_string())?;
    socket
        .send(&MachineFrame::Request {
            frame_id: frame.clone(),
            body: SessionRequest::Fleet(envelope(&request)?),
        })
        .await?;
    let expected_board = NoticeKind::BoardArchiving {
        board: BoardId::new(identity.board.as_str())?,
    };
    let expected_feed = NoticeKind::FleetFeedAdvanced {
        through: FleetPosition::new(6),
    };
    let mut observed = Vec::new();
    // Retain any hints that arrive before the correlated mutation answer;
    // there is no ordering assumption between the independent event roots.
    loop {
        match socket.receive().await? {
            CentralFrame::Answer {
                frame_id,
                body:
                    SessionAnswer::FleetDecision {
                        request_id,
                        decision,
                    },
            } if frame_id == frame => {
                equal(request_id.as_str(), request.request_id.as_str())?;
                check(
                    matches!(
                        serde_json::from_value::<FleetRequestResult>(decision.into_value())?,
                        FleetRequestResult::Decided(RequestDecision::Accepted { .. })
                    ),
                    "archive request accepted",
                )?;
                break;
            }
            CentralFrame::Notice { position, kind } => observed.push(Notice { position, kind }),
            other => return Err(format!("archive/notice correlation: {other:?}").into()),
        }
    }
    // A distinct supplied-clock event fires the actual platform Fleet alarm;
    // it does not make an inbound mutation dispatch down the DAG.
    client.set_time(at).await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !observed.iter().any(|notice| notice.kind == expected_board)
            || !observed.iter().any(|notice| notice.kind == expected_feed)
        {
            let CentralFrame::Notice { position, kind } = socket.receive().await? else {
                return Err("expected concrete delivered hint".into());
            };
            observed.push(Notice { position, kind });
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    let page = retained(&mut socket, at).await?;
    equal(page.len(), 3)?;
    equal(
        page.first().map(|notice| &notice.kind),
        Some(&NoticeKind::LeaseGranted {
            project,
            epoch: epoch(2)?,
        }),
    )?;
    check(
        page.iter().any(|notice| notice.kind == expected_board),
        "real known-holder board hint persisted",
    )?;
    check(
        page.iter()
            .any(|notice| notice.kind == expected_feed && notice.position.value() != 6),
        "real feed hint has a separate per-installation position",
    )?;
    socket.close().await?;
    println!(
        "NOTICES real Fleet alarm: registered reader + workspace archive, actual BoardArchiving/FleetFeedAdvanced delivered and locally read with independent positive notice positions: passed"
    );
    Ok(())
}
