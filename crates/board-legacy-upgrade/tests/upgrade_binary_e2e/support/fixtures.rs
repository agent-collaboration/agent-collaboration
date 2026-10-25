use super::{E2eResult, promote_error, promote_result};
use crate::fixture_support;
use board_legacy_upgrade::PreparedUpgrade;
use fleet_admission::ImportProvenance;
use message_board::{HumanId, MachineLabel, ServiceId};
use std::path::{Path, PathBuf};

pub struct CliImportFixture {
    pub directory: tempfile::TempDir,
    pub source: PathBuf,
    pub router_root: PathBuf,
    pub identity_file: PathBuf,
    pub report: PathBuf,
}

impl CliImportFixture {
    pub fn new(name: &str) -> E2eResult<Self> {
        let (directory, source) = promote_result(fixture_support::fixture_copy(name))?;
        let router_root = directory.path().join("router");
        std::fs::create_dir(&router_root)?;
        let identity_file = promote_result(fixture_support::create_host_identity(&router_root))?;
        let report = directory.path().join("report.json");
        Ok(Self {
            directory,
            source,
            router_root,
            identity_file,
            report,
        })
    }
}

pub fn project_copy(project_id: &str) -> String {
    format!("copy-{project_id}")
}

pub async fn prepare_for_cli(
    source: &Path,
    router_root: &Path,
    owner: &str,
    machine: &str,
) -> E2eResult<PreparedUpgrade> {
    let directory = codex_router_host::router_collaboration_directory(router_root);
    let installation = collaboration_service::read_service_identity(&directory)?
        .ok_or_else(|| promote_error("fixture Router identity is absent"))?;
    let provenance = ImportProvenance {
        installation: ServiceId::try_from(String::from(installation))?,
        machine: MachineLabel::try_from(machine.to_owned())?,
    };
    let owner = HumanId::try_from(owner.to_owned())?;
    Ok(board_legacy_upgrade::prepare_sources(&[source.to_owned()], provenance, owner).await?)
}
