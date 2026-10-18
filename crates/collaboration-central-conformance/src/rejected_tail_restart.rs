//! Historical-writer verification, retrievable evidence and a physical restart.
use crate::{ConformanceClient, ConformanceResult, HolderSession, holder_fixtures::*};
use collaboration_central_core::{AdminRequest, AdminResult, RegistryTime};
use collaboration_replication_protocol::*;

pub struct RejectedTailRestartScenario {
    successor: HolderFixture,
    report: TailReportRow,
    at: RegistryTime,
}

fn request(report: &TailReportRow) -> SessionRequest {
    SessionRequest::TailReport {
        project: report.project.clone(),
        epoch: report.epoch,
        request_ids: report.request_ids.clone(),
        digest: report.digest.clone(),
    }
}

async fn read(
    socket: &mut HolderSession,
    report: &TailReportRow,
    at: RegistryTime,
) -> ConformanceResult<()> {
    equal(
        socket
            .request(SessionRequest::Read {
                scope: ReadScope::TailReports {
                    project: report.project.clone(),
                },
                after: None,
            })
            .await?,
        SessionAnswer::ReadPage(ReadPage::TailReports {
            reports: vec![report.clone()],
            as_of: at.0,
        }),
    )
}

impl RejectedTailRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let (old, mut old_socket, _, project, at) = crate::notice_runtime::claimed(client).await?;
        let successor = HolderFixture::enroll(client, "tail successor").await?;
        let (mut socket, _) = successor.connect(client).await?;
        let AdminResult::Preview(preview) = client
            .admin(AdminRequest::TakeoverPreview {
                project: project.clone(),
            })
            .await?
        else {
            return Err("tail takeover preview".into());
        };
        equal(
            client
                .admin(AdminRequest::Takeover {
                    transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                    project: project.clone(),
                    to_installation: successor.installation.clone(),
                    reason: "tail historical writer".into(),
                    confirm_token: preview.confirm_token,
                })
                .await?,
            AdminResult::Granted {
                new_epoch: epoch(3)?,
                start: position(1, 3)?,
            },
        )?;
        let report = TailReportRow {
            project,
            epoch: epoch(2)?,
            request_ids: vec![
                RequestId::new(uuid::Uuid::now_v7().to_string())?,
                RequestId::new(uuid::Uuid::now_v7().to_string())?,
            ],
            digest: TailDigest::new("ab".repeat(32))?,
        };
        equal(
            old_socket.request(request(&report)).await?,
            SessionAnswer::TailStored,
        )?;
        let mut changed = report.clone();
        changed.request_ids.clear();
        equal(
            old_socket.request(request(&changed)).await?,
            SessionAnswer::TailStored,
        )?;
        changed.digest = TailDigest::new("cd".repeat(32))?;
        equal(
            old_socket.request(request(&changed)).await?,
            SessionAnswer::Refused(SessionRefusal::TailConflict {
                existing: report.digest.clone(),
            }),
        )?;
        let outsider = HolderFixture::enroll(client, "tail never writer").await?;
        let (mut outsider_socket, _) = outsider.connect(client).await?;
        equal(
            outsider_socket.request(request(&report)).await?,
            SessionAnswer::Refused(SessionRefusal::NotWriter {
                current: Some(epoch(3)?),
            }),
        )?;
        read(&mut socket, &report, at).await?;
        // Fleet's delivery root is a later platform event, not the report RPC.
        client.set_time(at).await?;
        let notices = crate::notice_runtime::retained(&mut socket, at).await?;
        equal(
            notices
                .iter()
                .filter(|notice| {
                    notice.kind
                        == NoticeKind::RejectedTailReported {
                            project: report.project.clone(),
                            epoch: report.epoch,
                        }
                })
                .count(),
            1,
        )?;
        outsider_socket.close().await?;
        old_socket.close().await?;
        socket.close().await?;
        // Keep the enrolled historical writer's identity distinct from the
        // authenticated successor that will read after restart.
        check(
            old.installation != successor.installation,
            "distinct tail holders",
        )?;
        Ok(Self {
            successor,
            report,
            at,
        })
    }

    pub async fn verify(self, client: &ConformanceClient) -> ConformanceResult<()> {
        let reopened = RegistryTime(self.at.0 + chrono::TimeDelta::seconds(1));
        client.set_time(reopened).await?;
        let (mut socket, projects) = self.successor.connect(client).await?;
        let expected_epoch = epoch(3)?;
        check(
            projects.iter().any(|state| {
                state.project == self.report.project && state.lease.epoch == expected_epoch
            }),
            "successor Welcome after physical restart",
        )?;
        read(&mut socket, &self.report, reopened).await?;
        equal(
            socket
                .request(SessionRequest::Read {
                    scope: ReadScope::TailReports {
                        project: self.report.project,
                    },
                    after: Some(ReadCursor::Fleet(FleetPosition::new(0))),
                })
                .await?,
            SessionAnswer::Refused(SessionRefusal::InvalidShape {
                field: "after".into(),
            }),
        )?;
        socket.close().await?;
        println!(
            "TAIL physical restart: real owner import/claim/takeover; historical writer stored, exact conflict/first content retained, never-writer refused, numbered hint and successor public read after restart: passed"
        );
        Ok(())
    }
}
