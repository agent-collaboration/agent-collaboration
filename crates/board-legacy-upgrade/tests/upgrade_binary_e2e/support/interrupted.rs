use super::{
    CentralHarness, CliImportFixture, E2eResult, prepare_for_cli, promote_error,
    reserve_first_project,
};
use board_legacy_upgrade::PreparedUpgrade;
use collaboration_central_core::AdminImportHeader;

pub struct InterruptedImport {
    pub fixture: CliImportFixture,
    pub central: CentralHarness,
    pub prepared: Option<PreparedUpgrade>,
    pub header: AdminImportHeader,
    pub project: String,
    pub copy: String,
    pub owner: String,
    pub machine: String,
}

impl InterruptedImport {
    pub async fn start(owner: &str, machine: &str) -> E2eResult<Self> {
        let fixture = CliImportFixture::new("legacy-complete.sqlite")?;
        let prepared =
            prepare_for_cli(&fixture.source, &fixture.router_root, owner, machine).await?;
        let central = match CentralHarness::start().await {
            Ok(central) => central,
            Err(error) => {
                prepared.close().await;
                return Err(error);
            }
        };
        let (header, project) = match reserve_first_project(&central, &prepared).await {
            Ok(reservation) => reservation,
            Err(error) => {
                prepared.close().await;
                let cleanup = central.shutdown().await;
                let detail = cleanup.map_or_else(
                    |cleanup| format!("{error}; central cleanup failed: {cleanup}"),
                    |_| error.to_string(),
                );
                return Err(promote_error(detail));
            }
        };
        let copy = super::project_copy(&project);
        Ok(Self {
            fixture,
            central,
            prepared: Some(prepared),
            header,
            project,
            copy,
            owner: owner.to_owned(),
            machine: machine.to_owned(),
        })
    }

    pub async fn shutdown(mut self) -> E2eResult<()> {
        if let Some(prepared) = self.prepared.take() {
            prepared.close().await;
        }
        self.central.shutdown().await
    }
}
