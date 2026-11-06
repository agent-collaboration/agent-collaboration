//! Debug-only, read-only hosted transport probe using the publisher's actual owner client.

#[cfg(debug_assertions)]
#[derive(serde::Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum StatusOutput {
    Missing,
    Progress {
        import_id: fleet_sync_protocol::RequestId,
        phase: collaboration_central_core::AdminImportPhase,
        digest: fleet_sync_protocol::ImportDigest,
        position: fleet_sync_protocol::ReplicationPosition,
    },
    Failed {
        reason: String,
        diagnostic: Option<board_staging_cutover::PublicationDiagnostic>,
    },
}

#[cfg(debug_assertions)]
async fn run() -> Result<std::process::ExitCode, Box<dyn std::error::Error>> {
    use board_staging_cutover::{PreparedProjectBundle, StagingPublisher};
    use std::io::Write;
    let mut arguments = std::env::args_os().skip(1);
    let target = arguments.next().ok_or("central origin required")?;
    let bundle = arguments.next().ok_or("sealed bundle directory required")?;
    if arguments.next().is_some() {
        return Err("usage: publication_status <central-origin> <sealed-bundle-directory>".into());
    }
    let target = url::Url::parse(target.to_str().ok_or("central origin must be UTF-8")?)?;
    let bundle = PreparedProjectBundle::open(std::path::Path::new(&bundle))?;
    let publisher = StagingPublisher::from_owner_access(target).await?;
    let (output, exit) = match publisher.inspect_import_status(&bundle).await {
        Ok(None) => (StatusOutput::Missing, std::process::ExitCode::SUCCESS),
        Ok(Some(record)) => (
            StatusOutput::Progress {
                import_id: record.import_id,
                phase: record.phase,
                digest: record.header.digest,
                position: record.header.manifest.position,
            },
            std::process::ExitCode::SUCCESS,
        ),
        Err(error) => (
            StatusOutput::Failed {
                reason: error.to_string(),
                diagnostic: error.diagnostic(),
            },
            std::process::ExitCode::FAILURE,
        ),
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output)?;
    stdout.write_all(b"\n")?;
    Ok(exit)
}

#[cfg(debug_assertions)]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(debug_assertions))]
fn main() -> std::process::ExitCode {
    eprintln!("publication_status is available only in debug builds");
    std::process::ExitCode::FAILURE
}
