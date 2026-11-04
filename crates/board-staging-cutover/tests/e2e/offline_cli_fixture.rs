//! An explicit preserved graph and existing private owner handoff for the CLI.
use crate::{cutover_host_fixture::CutoverHostFixture, native_snapshot_fixture::NativeFixture};
use codex_router_secret_store::central_credentials::CentralCredential;
use fleet_admission::{FleetImportBundle, FleetImportRecord, ImportProvenance, ImportedBoardState};
use message_board::RequestId;
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct RunFixture {
    pub plan: PathBuf,
    pub owner: PathBuf,
    pub bundle: PathBuf,
    pub claim: RequestId,
    pub canary: RequestId,
}

impl RunFixture {
    pub fn new(
        host: &CutoverHostFixture,
        source: &NativeFixture,
        target: &str,
        area: &Path,
    ) -> TestResult<Self> {
        std::fs::set_permissions(area, std::fs::Permissions::from_mode(0o700))?;
        let time = source.record_time;
        let workspace = source.scope.workspace.clone();
        let board = source.scope.board.clone().ok_or("source board absent")?;
        let fleet = FleetImportBundle {
            provenance: ImportProvenance {
                installation: String::from(host.service.clone()).try_into()?,
                machine: "compiled cutover source".to_owned().try_into()?,
            },
            declared_owner: uuid::Uuid::now_v7().to_string().try_into()?,
            records: vec![
                FleetImportRecord::Workspace {
                    id: workspace.clone(),
                    name: "preserved workspace".to_owned().try_into()?,
                    description: "unchanged".to_owned().try_into()?,
                    repositories: vec![],
                    time,
                },
                FleetImportRecord::Board {
                    id: board.clone(),
                    workspace,
                    name: "preserved board".to_owned().try_into()?,
                    description: "unchanged".to_owned().try_into()?,
                    state: ImportedBoardState::Active,
                    time,
                },
                FleetImportRecord::Directory {
                    project: source.selected.project.clone(),
                    board,
                    title: "preserved project".to_owned().try_into()?,
                    time,
                },
            ],
        };
        let run = uuid::Uuid::now_v7();
        let claim = RequestId::generate();
        let canary = RequestId::generate();
        let plan = area.join("run.json");
        std::fs::write(
            &plan,
            serde_json::to_vec(&json!({
                "runId":run, "routerRoot":host.root, "serviceId":host.service, "peerId":host.peer,
                "target":target, "bundleParent":area,
                "projects":[{"selected":source.selected,"fleet":fleet,"claimRequest":claim,"canaryRequest":canary}]
            }))?,
        )?;
        std::fs::set_permissions(&plan, std::fs::Permissions::from_mode(0o600))?;
        let fixture = Self {
            plan,
            owner: area.join("owner-handoff"),
            bundle: std::fs::canonicalize(area)?
                .join(format!("{run}-{}", source.selected.project.as_str())),
            claim,
            canary,
        };
        fixture.renew_owner(target)?;
        Ok(fixture)
    }

    pub fn renew_owner(&self, endpoint: &str) -> TestResult {
        CentralCredential::Owner {
            endpoint: endpoint.to_owned(),
            assertion: collaboration_central_conformance::owner_assertion()?,
            expires_at: "2099-01-01T00:00:00Z".to_owned(),
        }
        .write_private_file(&self.owner)?;
        Ok(())
    }

    pub async fn execute(&self) -> std::io::Result<std::process::Output> {
        self.command().output().await
    }

    pub async fn execute_claim(&self) -> std::io::Result<std::process::Output> {
        tokio::process::Command::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/cargo_debug_signing_runner.sh"),
        )
        .arg(env!("CARGO_BIN_EXE_codex-router-board-staging-cutover"))
        .arg("claim")
        .arg("--plan")
        .arg(&self.plan)
        .arg("--owner-credential-file")
        .arg(&self.owner)
        .kill_on_drop(true)
        .output()
        .await
    }

    pub fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/cargo_debug_signing_runner.sh"),
        );
        command
            .arg(env!("CARGO_BIN_EXE_codex-router-board-staging-cutover"))
            .arg("offline")
            .arg("--plan")
            .arg(&self.plan)
            .arg("--owner-credential-file")
            .arg(&self.owner)
            .kill_on_drop(true);
        command
    }
}
