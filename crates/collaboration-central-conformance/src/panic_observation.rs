use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, copy_scenarios::CopyFixture,
    holder_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;

/// G18 is an observation, not an assertion that a panic has a guessed rollback
/// outcome. The independent process restart is owned by the lifecycle harness.
pub struct PanicObservation {
    project: ProjectId,
}
impl PanicObservation {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let copy = CopyFixture::create(client).await?;
        client
            .control(ControlRequest::PanicAfterWrites {
                address: copy.address(),
                after_writes: 1,
            })
            .await?;
        let change = piece(
            &copy.project,
            position(1, 1)?,
            position(1, 2)?,
            vec![rows(2, vec![row(1, "panic write")])],
        )?;
        match tokio::time::timeout(std::time::Duration::from_secs(3), copy.apply(change)).await {
            Err(_) => println!("G18 request observation: native 3-second deadline elapsed"),
            Ok(Err(error)) => println!("G18 request observation: failed: {error}"),
            Ok(Ok(answer)) => println!("G18 request observation: returned {answer:?}"),
        }
        Ok(Self {
            project: copy.project,
        })
    }
    pub async fn record(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let address = ObjectAddress::Copy(self.project.clone());
        let status = client
            .call(
                address.clone(),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: self.project.clone(),
                }),
            )
            .await?;
        let WireResult::Copy(CopyResult::CopyStatus(Some(status))) = status else {
            return Err("G18 copy state could not be read after restart".into());
        };
        let result = client
            .call(
                address,
                WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
            )
            .await?;
        let WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
        ))) = result
        else {
            return Err("G18 rows could not be read after restart".into());
        };
        println!(
            "G18 persisted observation after restart: fence={}, prefix={:?}, table={}, rows={:?}",
            status.fence.value(),
            status.acked,
            page.table,
            page.rows
        );
        Ok(())
    }
}
