use crate::{ConformanceClient, ConformanceResult, ControlRequest, holder_fixtures::*};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub struct SessionResumeScenario {
    holder: HolderFixture,
    project: ProjectId,
    piece: Piece,
    renewal: SessionRequest,
    renewal_expiry: chrono::DateTime<chrono::Utc>,
}
impl SessionResumeScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        client.set_time(time()?).await?;
        let holder = HolderFixture::enroll(client, "whole-server-restart").await?;
        let (mut socket, _) = holder.connect(client).await?;
        let project = project()?;
        client
            .call(
                ObjectAddress::Registry,
                WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                    project: project.clone(),
                    source: ReservationSource::FleetAcceptance {
                        installation: holder.installation.clone(),
                    },
                }),
            )
            .await?;
        let piece = piece(
            &project,
            position(1, 0)?,
            position(1, 1)?,
            vec![schema_transaction()],
        )?;
        client
            .control(ControlRequest::DropNextAnswer {
                installation: holder.installation.clone(),
            })
            .await?;
        socket
            .send(&MachineFrame::Request {
                frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
                body: SessionRequest::Piece(piece.clone()),
            })
            .await?;
        client
            .control(ControlRequest::AwaitDroppedAnswer {
                installation: holder.installation.clone(),
            })
            .await?;
        let renewal = SessionRequest::Renew {
            attempt: RequestId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            epoch: epoch(1)?,
        };
        client
            .control(ControlRequest::DropNextAnswer {
                installation: holder.installation.clone(),
            })
            .await?;
        socket
            .send(&MachineFrame::Request {
                frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
                body: renewal.clone(),
            })
            .await?;
        client
            .control(ControlRequest::AwaitDroppedAnswer {
                installation: holder.installation.clone(),
            })
            .await?;
        socket.close().await?;
        // Observe current lease expiry without replaying the attempt or reading
        // either original answer. After the real restart its receipt must agree.
        let (observed, projects) = holder.connect(client).await?;
        let renewal_expiry = projects
            .iter()
            .find(|state| state.project == project)
            .ok_or("prepared renewal project missing from Welcome")?
            .lease
            .expires_at_registry;
        observed.close().await?;
        println!(
            "STAGING S3 prepared project={}: piece and renewal answers dropped; Welcome expiry={} recorded, attempt not replayed",
            project.as_str(),
            renewal_expiry
        );
        Ok(Self {
            holder,
            project,
            piece,
            renewal,
            renewal_expiry,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        client.set_time(time()?).await?;
        let (mut socket, projects) = self.holder.connect(client).await?;
        let expected = position(1, 1)?;
        check(
            projects.iter().any(|state| {
                state.project == self.project
                    && state.acked == expected
                    && state.lease.epoch.value() == 1
            }),
            "whole-server restart Welcome must recover committed ack/lease",
        )?;
        equal(
            socket
                .request(SessionRequest::Piece(self.piece.clone()))
                .await?,
            SessionAnswer::Acked { through: expected },
        )?;
        check(
            matches!(socket.request(SessionRequest::Read{scope:ReadScope::ProjectCopy{project:self.project.clone()},after:None}).await?,SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page)))if page.schema.version==1&&page.position==expected&&page.rows.is_empty()),
            "replayed schema does not repeat after server restart",
        )?;
        let mut previous_remaining = None;
        for _ in 0..2 {
            let SessionAnswer::Renewed {
                expires_at,
                transfer_in_progress: None,
                replayed: true,
                current,
            } = socket.request(self.renewal.clone()).await?
            else {
                return Err("dropped renewal must replay without an active transfer".into());
            };
            check(
                expires_at == self.renewal_expiry
                    && current.expires_at_registry == self.renewal_expiry
                    && current.epoch == epoch(1)?
                    && current.holder == self.holder.installation
                    && current.state == LeaseState::Held,
                "dropped renewal receipt retained its original expiry/authority after restart",
            )?;
            if let Some(remaining) = previous_remaining {
                check(
                    current.remaining_ms <= remaining,
                    "replaying a renewal must not extend its remaining lifetime",
                )?;
            }
            previous_remaining = Some(current.remaining_ms);
        }
        println!(
            "STAGING S3 project={}: original ack and renewal expiry={} recovered, unchanged attempt replayed twice",
            self.project.as_str(),
            self.renewal_expiry
        );
        socket.close().await
    }
}
