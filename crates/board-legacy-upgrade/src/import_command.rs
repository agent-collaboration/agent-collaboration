//! Input preparation precedes credentials and any owner-only central request.
use board_legacy_upgrade::{
    IMPORT_DIGEST_BASIS, MessageUpgrade, MessageUpgradeReport, UpgradeReport, UpgradeUploadReport,
    prepare_sources,
};
use clap::Parser;
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, router_collaboration_directory};
use fleet_admission::ImportProvenance;
use message_board::{HumanId, MachineLabel, ServiceId};
use serde::Serialize;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "codex-router-board-upgrade",
    version,
    about = "Upgrade a closed legacy board store through central's owner import"
)]
pub struct ImportCommand {
    #[arg(long, required = true)]
    source: Vec<PathBuf>,
    #[arg(long)]
    report: PathBuf,
    /// Existing Router root; its Host identity is read without creating it.
    #[arg(long)]
    router_directory: PathBuf,
    #[arg(long)]
    owner_human_id: Option<String>,
    #[arg(long)]
    machine_label: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, required_unless_present = "dry_run")]
    central_url: Option<reqwest::Url>,
    /// Read the short-lived owner credential minted on this machine.
    #[arg(long, conflicts_with = "owner_credential_file")]
    self_hosted_root: Option<PathBuf>,
    /// Private owner credential handoff; consumed and removed before publication.
    #[arg(long)]
    owner_credential_file: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum RunReport<'report> {
    DryRun {
        sources: InputSources,
        digest_basis: &'static str,
        report: &'report UpgradeReport,
        messages: MessageUpgradeReport,
    },
    Uploaded {
        sources: InputSources,
        digest_basis: &'static str,
        report: &'report UpgradeReport,
        publication: UpgradeUploadReport,
        messages: MessageUpgradeReport,
    },
    Refused {
        reason: String,
        report: Option<&'report UpgradeReport>,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InputSources {
    installation: &'static str,
    owner: ValueSource,
    machine_label: ValueSource,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum ValueSource {
    ExplicitOverride,
    OsAccount,
    OfflineRouterDerivation,
}

impl ImportCommand {
    pub async fn execute(self) -> Result<bool, Box<dyn std::error::Error>> {
        self.validate_output()?;
        // Dry runs own only disposable output. The source Router's identity and
        // authority remain untouched, even when its Host is currently running.
        let disposable_directory = self.dry_run.then(tempfile::tempdir).transpose()?;
        let authority_directory = disposable_directory
            .as_ref()
            .map_or(self.router_directory.as_path(), |directory| {
                directory.path()
            });
        let coordination = HostCoordinationPaths::for_router_root(authority_directory);
        let _authority = HostInstanceLock::acquire(&coordination)?;
        let result = self.prepare().await;
        let (report, success) = match result {
            Ok((prepared, sources, messages)) => {
                let completed = async {
                    if self.dry_run {
                        let messages = messages
                            .copy_to(&authority_directory.join("interaction.sqlite"))
                            .await?;
                        Ok::<_, Box<dyn std::error::Error>>(serde_json::to_vec_pretty(
                            &RunReport::DryRun {
                                sources,
                                digest_basis: IMPORT_DIGEST_BASIS,
                                report: &prepared.report,
                                messages,
                            },
                        )?)
                    } else {
                        let publication = self.publish(&prepared).await?;
                        let destination = router_collaboration_directory(&self.router_directory)
                            .join("interaction.sqlite");
                        messages.copy_to(&destination).await?;
                        let messages = messages.apply_drop(&destination).await?;
                        Ok(serde_json::to_vec_pretty(&RunReport::Uploaded {
                            sources,
                            digest_basis: IMPORT_DIGEST_BASIS,
                            report: &prepared.report,
                            publication,
                            messages,
                        })?)
                    }
                }
                .await;
                let (encoded, success) = match completed {
                    Ok(encoded) => (Ok(encoded), true),
                    Err(error) => (
                        serde_json::to_vec_pretty(&RunReport::Refused {
                            reason: error.to_string(),
                            report: Some(&prepared.report),
                        }),
                        false,
                    ),
                };
                // The report owns its data now; explicitly close native resources before
                // the disposable directory is removed, including on report-write errors.
                prepared.close().await;
                (encoded?, success)
            }
            Err(error) => (
                serde_json::to_vec_pretty(&RunReport::Refused {
                    reason: error.to_string(),
                    report: None,
                })?,
                false,
            ),
        };
        std::fs::write(&self.report, report)?;
        Ok(success)
    }

    async fn publish(
        &self,
        prepared: &board_legacy_upgrade::PreparedUpgrade,
    ) -> Result<UpgradeUploadReport, Box<dyn std::error::Error>> {
        prepared.ensure_publishable()?;
        let endpoint = self
            .central_url
            .clone()
            .ok_or_else(|| io::Error::other("--central-url is required for publication"))?;
        let publisher = crate::import_access::publisher(
            endpoint,
            self.self_hosted_root.clone(),
            self.owner_credential_file.clone(),
        )
        .await?;
        Ok(publisher.publish(prepared).await?)
    }

    async fn prepare(
        &self,
    ) -> Result<
        (
            board_legacy_upgrade::PreparedUpgrade,
            InputSources,
            MessageUpgrade,
        ),
        Box<dyn std::error::Error>,
    > {
        let service_directory = router_collaboration_directory(&self.router_directory);
        let installation = collaboration_service::read_service_identity(&service_directory)?
            .ok_or_else(|| {
                io::Error::other("Router identity is absent; it must not be minted by the importer")
            })?;
        let (owner, owner_source) = match &self.owner_human_id {
            Some(value) => (
                HumanId::try_from(value.clone())?,
                ValueSource::ExplicitOverride,
            ),
            None => (
                collaboration_client::resolve_owner_human_id().await?,
                ValueSource::OsAccount,
            ),
        };
        let (label, label_source) = match &self.machine_label {
            Some(value) => (
                MachineLabel::try_from(value.clone())?,
                ValueSource::ExplicitOverride,
            ),
            None => {
                let identity =
                    collaboration_service::MachineIdentity::new(installation.clone(), None)?;
                (
                    MachineLabel::try_from(identity.machine_label().as_str().to_owned())?,
                    ValueSource::OfflineRouterDerivation,
                )
            }
        };
        let provenance = ImportProvenance {
            installation: ServiceId::try_from(String::from(installation))?,
            machine: label,
        };
        let prepared = prepare_sources(&self.source, provenance, owner).await?;
        let messages = MessageUpgrade::prepare(
            &service_directory.join("automation.sqlite"),
            &prepared.message_activity,
        )
        .await;
        let messages = match messages {
            Ok(messages) => messages,
            Err(error) => {
                prepared.close().await;
                return Err(error.into());
            }
        };
        Ok((
            prepared,
            InputSources {
                installation: "existingRouterIdentity",
                owner: owner_source,
                machine_label: label_source,
            },
            messages,
        ))
    }

    fn validate_output(&self) -> io::Result<()> {
        // Reserve destination paths before their first owning open. Inode
        // identity also catches hard links to stores that already exist.
        use std::os::unix::fs::MetadataExt;
        if std::fs::symlink_metadata(&self.report)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(io::Error::other("report path must not be a symbolic link"));
        }
        let output_path = resolved_output_path(&self.report)?;
        let output_metadata = std::fs::metadata(&self.report).ok();
        let service_directory = router_collaboration_directory(&self.router_directory);
        let reserved = [
            service_directory.join("service-identity.json"),
            service_directory.join("automation.sqlite"),
            service_directory.join("interaction.sqlite"),
        ];
        for input in self.source.iter().chain(&reserved) {
            let same_path = resolved_output_path(input).is_ok_and(|input| input == output_path);
            let same_inode = output_metadata.as_ref().is_some_and(|output| {
                std::fs::metadata(input)
                    .is_ok_and(|input| input.dev() == output.dev() && input.ino() == output.ino())
            });
            if same_path || same_inode {
                return Err(io::Error::other(
                    "report would overwrite an input store, message store or Router identity",
                ));
            }
        }
        Ok(())
    }
}

fn resolved_output_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("report and input paths must name files"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.canonicalize()?.join(name))
}
