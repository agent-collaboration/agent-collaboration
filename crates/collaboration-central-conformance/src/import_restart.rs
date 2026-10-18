use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, holder_fixtures::*, import_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub struct ImportClaimRestartScenario {
    fixture: ImportedProjectFixture,
    holder: HolderFixture,
    attempt: RequestId,
}
impl ImportClaimRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        client.set_time(time()?).await?;
        let fixture = ImportedProjectFixture::new()?;
        crate::fleet_directory_fixture::import_identity(client, &fixture.project).await?;
        fixture.upload(client).await?;
        let holder = HolderFixture::enroll(client, "claim-restart").await?;
        let (mut socket, _) = holder.connect(client).await?;
        let attempt = attempt()?;
        client.control(ControlRequest::InterruptAfterFence).await?;
        check(
            socket
                .request(claim_request(&fixture.project, attempt.clone()))
                .await
                .is_err(),
            "claim interrupted after fence before answer",
        )?;
        drop(socket);
        let state = fixture.state(client).await?;
        equal(state.fence, epoch(2)?)?;
        equal(state.holder, Some(holder.installation.clone()))?;
        Ok(Self {
            fixture,
            holder,
            attempt,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        client.set_time(time()?).await?;
        let (mut socket, welcome) = self.holder.connect(client).await?;
        check(
            welcome
                .iter()
                .all(|state| state.project != self.fixture.project),
            "interrupted fence has not granted a registry lease",
        )?;
        let expected = SessionAnswer::Claimed {
            epoch: epoch(2)?,
            expires_at: time()?.0 + chrono::TimeDelta::hours(4),
            start: position(1, 3)?,
        };
        equal(
            socket
                .request(claim_request(&self.fixture.project, self.attempt.clone()))
                .await?,
            expected.clone(),
        )?;
        client
            .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::seconds(60)))
            .await?;
        equal(
            socket
                .request(claim_request(&self.fixture.project, self.attempt.clone()))
                .await?,
            expected,
        )?;
        equal(self.fixture.state(client).await?.fence, epoch(2)?)?;
        socket.close().await?;
        println!(
            "IMPORT restart: copy fenced before grant; replay after fresh server/workerd grants the same epoch 2 once, then replays the original expiry and start"
        );
        Ok(())
    }
}
