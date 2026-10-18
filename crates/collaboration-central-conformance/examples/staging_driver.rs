#![forbid(unsafe_code)]
//! Runs M3.10's staging rows against a deployed central behind real
//! Cloudflare Access. Hostnames, the redeploy command and the private log path
//! come from the private ops repository; credentials come from the invoking
//! command's environment and the owner's `cloudflared` sign-in. Nothing here
//! prints any of them.
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, DeploySentinel, PanicObservation, SessionResumeScenario,
    StagingAccess, TransactionRestartScenario, TransferRestartScenario, capture_machine_assertion,
    guard_fault_target, guard_normal_target, owner_app_token, prepare_staging_normal_rows,
    run_staging_access_rows, run_staging_expiry_row, run_staging_hibernation_rows,
    run_staging_renewal_rows, run_staging_replacement_row, run_staging_revocation_row,
    run_staging_session_conformance, run_staging_verifier_rows,
};
use collaboration_central_core::{AdminRefusal, AdminRequest, AdminResult};
use collaboration_replication_protocol::{
    Epoch, ImportDigestBuilder, ImportSnapshotHeader, ProjectId, ProjectImportAnswer,
    ProjectImportPage, ProjectSummary, ReplicationPosition, SchemaIdentity, SnapshotPage,
    schema_fingerprint,
};
use std::{sync::Arc, time::Duration};

const USAGE: &str = "usage: staging_driver enroll <central-url> <admin-app-url>
       staging_driver owner-preflight <central-url> <admin-app-url>
       staging_driver verifier <central-url> <admin-app-url> <inventory-host> [other-admin-app-url]
       staging_driver fault <central-url> <admin-app-url> <inventory-host> <redeploy-command> <private-log>
       staging_driver fault-overlap <central-url> <admin-app-url> <inventory-host>
       staging_driver fault-unproven <central-url> <admin-app-url> <inventory-host>
       staging_driver fault-resume <central-url> <admin-app-url> <inventory-host> <redeploy-command> <private-log>
       staging_driver normal <central-url> <admin-app-url> <inventory-host> <redeploy-command> <private-log>
       staging_driver hibernation <central-url> <admin-app-url> <inventory-host> <idle-seconds>...
       staging_driver expiry <central-url> <admin-app-url> <inventory-host>";

/// How long a deploy may take to close the sentinel socket before the run
/// reports that the socket survived.
const DEPLOY_CLOSE_BOUND: Duration = Duration::from_secs(180);

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or(USAGE)?;
    let central_url = args.next().ok_or(USAGE)?;
    let admin_app_url = args.next().ok_or(USAGE)?;
    let access = Arc::new(StagingAccess::from_environment(&admin_app_url).await?);
    let client = ConformanceClient::staging(&central_url, access)?;
    match mode.as_str() {
        "enroll" => enroll(&client).await,
        "owner-preflight" => owner_preflight(&client).await,
        "verifier" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            guard_fault_target(&client, &inventory_host).await?;
            let machine = capture_machine_assertion(&client, "old").await?;
            let other = match args.next() {
                Some(url) => Some(owner_app_token(&url).await?),
                None => None,
            };
            run_staging_verifier_rows(&client, machine, other).await
        }
        "fault" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            let redeploy_command = args.next().ok_or(USAGE)?;
            let private_log = args.next().ok_or(USAGE)?;
            fault(&client, &inventory_host, &redeploy_command, &private_log).await
        }
        "fault-overlap" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            guard_fault_target(&client, &inventory_host).await?;
            collaboration_central_conformance::held_fence_and_piece_resolve_to_one_complete_prefix(
                &client,
            )
            .await
        }
        "fault-unproven" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            guard_fault_target(&client, &inventory_host).await?;
            collaboration_central_conformance::run_unproven_transfer_rows(&client).await
        }
        "fault-resume" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            let redeploy_command = args.next().ok_or(USAGE)?;
            let private_log = args.next().ok_or(USAGE)?;
            guard_fault_target(&client, &inventory_host).await?;
            let scenario = SessionResumeScenario::prepare(&client).await?;
            let sentinel = DeploySentinel::open(&client).await?;
            println!("STAGING S3 redeploy started at {}", chrono::Utc::now());
            redeploy(&redeploy_command, &private_log).await?;
            let closed = sentinel.await_close(DEPLOY_CLOSE_BOUND).await?;
            println!(
                "STAGING S3 redeploy finished at {}; socket closed={closed}",
                chrono::Utc::now()
            );
            scenario.verify(&client).await
        }
        "expiry" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            guard_fault_target(&client, &inventory_host).await?;
            let inner = capture_machine_assertion(&client, "old").await?;
            let outer = client
                .staging_access()
                .ok_or("expiry needs real Access")?
                .owner_app_token
                .clone();
            run_staging_expiry_row(&client, inner, outer).await
        }
        "hibernation" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            guard_normal_target(&client, &inventory_host).await?;
            let windows = args
                .map(|seconds| seconds.parse().map(Duration::from_secs))
                .collect::<Result<Vec<_>, _>>()?;
            if windows.is_empty() {
                return Err(USAGE.into());
            }
            run_staging_hibernation_rows(&client, &windows).await
        }
        "normal" => {
            let inventory_host = args.next().ok_or(USAGE)?;
            let redeploy_command = args.next().ok_or(USAGE)?;
            let private_log = args.next().ok_or(USAGE)?;
            normal(&client, &inventory_host, &redeploy_command, &private_log).await
        }
        _ => Err(USAGE.into()),
    }
}

/// Checks the real owner path without registering or replacing a machine session.
async fn owner_preflight(client: &ConformanceClient) -> ConformanceResult<()> {
    let project = ProjectId::new(uuid::Uuid::now_v7().to_string())?;
    let head = ReplicationPosition::new(Epoch::new(1)?, 1.into());
    let schema = SchemaIdentity::new(1, schema_fingerprint(&[])?)?;
    let summary = ProjectSummary {
        status: "imported".into(),
        latest_record_at: chrono::Utc::now(),
    };
    let digest = ImportDigestBuilder::new(&ImportSnapshotHeader {
        project: project.clone(),
        head,
        schema: schema.clone(),
        schema_steps: Vec::new(),
    })?
    .finish(&summary)?;
    let upload = ProjectImportPage {
        digest,
        index: 0,
        page: SnapshotPage {
            project: project.clone(),
            position: head,
            schema,
            schema_steps: Vec::new(),
            table: String::new(),
            rows: Vec::new(),
            next: None,
            as_of: chrono::Utc::now(),
        },
        final_summary: Some(summary),
    };
    println!("STAGING owner preflight project: {}", project.as_str());
    let imported = client.import_project(&project, &upload).await?;
    if imported != (ProjectImportAnswer::Imported { head }) {
        return Err(format!("owner preflight import: {imported:?}").into());
    }
    println!("STAGING owner preflight import: Imported at (1,1)");
    let preview = client
        .admin(AdminRequest::TakeoverPreview { project })
        .await?;
    if preview != AdminResult::Refused(AdminRefusal::NotInitialized) {
        return Err("owner preflight preview: expected unheld NotInitialized".into());
    }
    println!("STAGING owner preflight preview: Refused(NotInitialized), unheld import");
    Ok(())
}

/// G5: the owner enrolls both staging installations through the admin route,
/// each bound to its own Access service token and machine credential.
async fn enroll(client: &ConformanceClient) -> ConformanceResult<()> {
    use sha2::{Digest, Sha256};
    let access = client
        .staging_access()
        .ok_or("enrollment needs real Access")?;
    for installation in access.installations() {
        let result = client
            .admin(AdminRequest::EnrollMachine {
                label: installation.label.clone(),
                installation: installation.installation.clone(),
                access_client_id: installation.access_client_id.clone(),
                credential_hash: format!(
                    "{:x}",
                    Sha256::digest(installation.machine_credential.as_bytes())
                ),
                expires_at: chrono::Utc::now() + chrono::TimeDelta::days(90),
            })
            .await?;
        if result != AdminResult::MachineEnrolled {
            return Err(format!("enrollment of {} answered {result:?}", installation.label).into());
        }
        println!("STAGING enroll {}: MachineEnrolled", installation.label);
    }
    Ok(())
}

/// The fault central's crash, recovery and storage-transaction rows, prepared,
/// carried across a real redeploy of the same artifact, then verified.
async fn fault(
    client: &ConformanceClient,
    inventory_host: &str,
    redeploy_command: &str,
    private_log: &str,
) -> ConformanceResult<()> {
    guard_fault_target(client, inventory_host).await?;
    println!(
        "STAGING fault target guard: passed at {}",
        chrono::Utc::now()
    );
    run_staging_session_conformance(client).await?;
    if client
        .staging_access()
        .is_some_and(|access| access.temporary.is_some())
    {
        run_staging_replacement_row(client).await?;
    } else {
        println!("STAGING S4: NOT OBSERVED, no temporary installation configured");
    }
    let transaction = TransactionRestartScenario::prepare(client).await?;
    let transfer = TransferRestartScenario::prepare(client).await?;
    let session = SessionResumeScenario::prepare(client).await?;
    let panic = PanicObservation::prepare(client).await?;
    let sentinel = DeploySentinel::open(client).await?;
    println!("STAGING redeploy started at {}", chrono::Utc::now());
    redeploy(redeploy_command, private_log).await?;
    let closed = sentinel.await_close(DEPLOY_CLOSE_BOUND).await?;
    println!(
        "STAGING redeploy finished at {}; open session {}",
        chrono::Utc::now(),
        if closed {
            "closed by the deploy"
        } else {
            "still open at the bound"
        }
    );
    transaction.verify(client).await?;
    println!("STAGING X1-X3, X5 after redeploy: rollbacks, fence and complete contents: passed");
    transfer.verify(client).await?;
    println!("STAGING C1-C2 after redeploy: interrupted fence replays one grant: passed");
    session.verify(client).await?;
    println!("STAGING S3 after redeploy: lost answer recovered from Welcome and replay: passed");
    panic.record(client).await?;
    // The caller runs fault-unproven only after independent observers have
    // completed their postdeploy reads: C7 intentionally leaves missing copies.
    println!("STAGING C7: run fault-unproven last, after observer release");
    Ok(())
}

/// The normal central's Access, fence, takeover and resume rows, with a real
/// redeploy of the same guarded artifact while a piece is in flight.
async fn normal(
    client: &ConformanceClient,
    inventory_host: &str,
    redeploy_command: &str,
    private_log: &str,
) -> ConformanceResult<()> {
    guard_normal_target(client, inventory_host).await?;
    println!(
        "STAGING normal target guard: passed at {}",
        chrono::Utc::now()
    );
    run_staging_access_rows(client).await?;
    collaboration_central_conformance::run_staging_transport_rows(client).await?;
    run_staging_renewal_rows(client).await?;
    let mut state = prepare_staging_normal_rows(client).await?;
    println!("STAGING redeploy started at {}", chrono::Utc::now());
    redeploy(redeploy_command, private_log).await?;
    let closed = state.await_deploy_close(DEPLOY_CLOSE_BOUND).await?;
    println!(
        "STAGING redeploy finished at {}; open session {}",
        chrono::Utc::now(),
        if closed {
            "closed by the deploy"
        } else {
            "still open at the bound"
        }
    );
    state.verify(client).await?;
    if client
        .staging_access()
        .is_some_and(|access| access.temporary.is_some())
    {
        run_staging_revocation_row(client).await
    } else {
        println!("STAGING A10: NOT OBSERVED, no temporary installation configured");
        Ok(())
    }
}

/// Runs the private redeploy command with its output kept in the private log,
/// since a deploy prints hostnames and version identifiers.
async fn redeploy(command: &str, private_log: &str) -> ConformanceResult<()> {
    let log = std::fs::File::create(private_log)?;
    let status = tokio::process::Command::new(command)
        .stdout(log.try_clone()?)
        .stderr(log)
        .status()
        .await?;
    if !status.success() {
        return Err("redeploy command failed; see its private log".into());
    }
    Ok(())
}
