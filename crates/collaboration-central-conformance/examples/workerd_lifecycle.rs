#![forbid(unsafe_code)]
#[path = "support/workerd_process.rs"]
mod workerd_process;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, ForwardRestartScenario, ImportClaimRestartScenario,
    ImportDirectoryRestartScenario, JwksFixtureServer, NumericRestartScenario, PanicObservation,
    SessionResumeScenario, TransactionRestartScenario, TransferRestartScenario,
    run_access_conformance, run_access_verifier_control_conformance, run_copy_conformance,
    run_default_smoke, run_forward_conformance, run_forward_roundtrip_and_race,
    run_import_claim_conformance, run_import_claim_roundtrip, run_numeric_conformance,
    run_session_conformance,
};
use workerd_process::{WorkerdProcess, WorkerdSettings};

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .ok_or("default, conformance, dag-edge, forward-app or import-app mode")?;
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        node: args.next().ok_or("Node executable")?.into(),
        miniflare: args.next().ok_or("pinned Miniflare entrypoint")?.into(),
        persistence: std::path::PathBuf::from(args.next().ok_or("isolated persistence directory")?),
        port: args.next().ok_or("loopback port")?.parse()?,
        jwks_url: jwks.url.clone(),
    };
    let client = ConformanceClient::new(&format!("http://127.0.0.1:{}", settings.port))?;
    let mut server = WorkerdProcess::start(&settings).await?;
    if mode == "tail-restart" {
        let prepared =
            collaboration_central_conformance::RejectedTailRestartScenario::prepare(&client).await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if matches!(mode.as_str(), "fleet-outbox-restart" | "fleet-outbox-ack") {
        let loss = if mode == "fleet-outbox-restart" {
            collaboration_central_conformance::NoticeDeliveryLoss::Dispatch
        } else {
            collaboration_central_conformance::NoticeDeliveryLoss::Acknowledgement
        };
        let prepared =
            collaboration_central_conformance::FleetOutboxRestartScenario::prepare(&client, loss)
                .await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "notice-restart" {
        let prepared =
            collaboration_central_conformance::NoticeRestartScenario::prepare(&client).await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "fleet-notices" {
        let proof = collaboration_central_conformance::run_fleet_notice_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "fleet-feed-restart" {
        let prepared =
            collaboration_central_conformance::FleetFeedRestartScenario::prepare(&client).await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let pruned = prepared.until_pruned(&client).await;
        restarted.stop().await?;
        let pruned = pruned?;
        let after_pruning = WorkerdProcess::start(&settings).await?;
        let proof = pruned.append(&client).await;
        after_pruning.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "lease-order-restart" {
        let prepared =
            collaboration_central_conformance::LeaseOrderRestartScenario::prepare(&client).await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if matches!(mode.as_str(), "archive-cleanup" | "archive-cleanup-ack") {
        let loss = if mode == "archive-cleanup" {
            collaboration_central_conformance::CleanupLoss::Dispatch
        } else {
            collaboration_central_conformance::CleanupLoss::Acknowledgement
        };
        let prepared = collaboration_central_conformance::ArchiveCleanupRestartScenario::prepare(
            &client, loss,
        )
        .await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "archive" {
        let proof =
            collaboration_central_conformance::run_archive_report_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "directory-restart" {
        let prepared = ImportDirectoryRestartScenario::prepare(&client).await;
        server.stop().await?;
        let prepared = prepared?;
        let restarted = WorkerdProcess::start(&settings).await?;
        let proof = prepared.verify(&client).await;
        restarted.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "fleet-conflict" {
        let proof =
            collaboration_central_conformance::run_fleet_conflict_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "fleet-requests" {
        let proof = collaboration_central_conformance::run_fleet_request_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "fleet-import" {
        let proof = collaboration_central_conformance::run_fleet_import_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "malformed" {
        let proof =
            collaboration_central_conformance::run_malformed_frame_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "default" {
        let proof = async {
            run_access_conformance(&client, &jwks.requests).await?;
            run_default_smoke(&client).await?;
            collaboration_central_conformance::run_staging_transport_rows(&client).await
        }
        .await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "dag-edge" {
        let proof = collaboration_central_conformance::run_call_edge_conformance(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "import-app" {
        let proof = run_import_claim_roundtrip(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode == "forward-app" {
        let proof = run_forward_roundtrip_and_race(&client).await;
        server.stop().await?;
        jwks.shutdown().await?;
        return proof;
    }
    if mode != "conformance" {
        server.stop().await?;
        jwks.shutdown().await?;
        return Err(
            "expected default, conformance, dag-edge, forward-app or import-app mode".into(),
        );
    }
    let prepared = async {
        run_import_claim_conformance(&client).await?;
        run_forward_conformance(&client).await?;
        run_access_conformance(&client, &jwks.requests).await?;
        run_access_verifier_control_conformance(&client).await?;
        access_log_layers(&client, &mut server).await?;
        run_copy_conformance(&client).await?;
        run_numeric_conformance(&client).await?;
        run_session_conformance(&client).await?;
        collaboration_central_conformance::run_archive_report_conformance(&client).await?;
        let import_directory = ImportDirectoryRestartScenario::prepare(&client).await?;
        let import_claim = ImportClaimRestartScenario::prepare(&client).await?;
        let transaction = TransactionRestartScenario::prepare(&client).await?;
        let numeric = NumericRestartScenario::prepare(&client).await?;
        let transfer = TransferRestartScenario::prepare(&client).await?;
        let session = SessionResumeScenario::prepare(&client).await?;
        let forward_restart = ForwardRestartScenario::prepare(&client).await?;
        let panic = PanicObservation::prepare(&client).await?;
        server.await_panic().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((
            transaction,
            numeric,
            transfer,
            session,
            panic,
            forward_restart,
            import_directory,
            import_claim,
        ))
    }
    .await;
    let registry_before = instance_labels(&server.central_lines(), "registry");
    server.stop().await?;
    let (
        transaction,
        numeric,
        transfer,
        session,
        panic,
        forward_restart,
        import_directory,
        import_claim,
    ) = prepared?;
    let restarted = WorkerdProcess::start(&settings).await?;
    let verified=async {
        forward_restart.verify(&client).await?;
        // The forward's local deadline intentionally makes no Registry call.
        // Wake it explicitly, then join its real constructor observation.
        client.control(collaboration_central_conformance::ControlRequest::PendingAlarms).await?;
        let registry_lines=restarted.await_object_instance("registry").await?;
        let registry_after=instance_labels(&registry_lines,"registry");
        if registry_before.is_empty()
            || registry_after.is_empty()
            || !registry_before.is_disjoint(&registry_after)
        {
            return Err("a restarted registry must log a new instance label".into());
        }
        println!(
            "INSTANCE labels: registry {} before and {} after the physical restart, none shared: passed",
            registry_before.len(),
            registry_after.len()
        );
        transaction.verify(&client).await?;
        println!("RESTART SQL constraint/Rust Err rollback, fence mismatch/matching epoch, concurrent reads see old or complete new contents: passed");
        numeric.verify(&client).await?;
        println!("RESTART exact integer and REAL storage classes, snapshot and original receipt: passed");
        transfer.verify(&client).await?;
        println!("RESTART interrupted fence, same-transfer replay grants only one epoch, stale old/new holder: passed");
        session.verify(&client).await?;
        println!("RESTART lost answer, authenticated reconnect, Welcome reconciliation and replay: passed");
        import_claim.verify(&client).await?;
        import_directory.verify(&client).await?;
        panic.record(&client).await?;
        Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    restarted.stop().await?;
    jwks.shutdown().await?;
    verified?;
    println!(
        "WORKERD local: 8 shared groups and 7 physical-restart groups passed; staging/deploy/TLS unverified"
    );
    Ok(())
}

/// P2 on real workerd. In one control request the edge checks the owner and
/// the control checks the submitted assertion; each logs its own layer under
/// the request's invocation label. A flipped signature byte must be logged
/// refused at the inner layer while the outer owner check is accepted, and
/// the unmodified assertion accepted, so success is logged only after
/// WebCrypto validates.
async fn access_log_layers(
    client: &ConformanceClient,
    server: &mut WorkerdProcess,
) -> ConformanceResult<()> {
    use collaboration_central_conformance::{
        AccessCallerClass, AccessVerificationOutcome, flip_signature_byte, owner_assertion,
    };
    let admin = "/admin/v1/machines/enroll";
    let bound = std::time::Duration::from_secs(10);
    let owner = owner_assertion()?;
    for (assertion, inner_outcome) in [
        (flip_signature_byte(&owner)?, "outcome=refused BadSignature"),
        (owner.clone(), "outcome=accepted owner"),
    ] {
        let from = server.central_lines().len();
        let answer = client.verify_access_assertion(assertion, admin).await?;
        let inner = server
            .await_central_line(from, bound, |line| {
                line.starts_with("central access:") && line.contains(" layer=control-inner ")
            })
            .await?;
        if !inner.ends_with(inner_outcome) {
            return Err(format!("inner layer logged {inner:?}, answered {answer:?}").into());
        }
        let invocation = inner
            .split_whitespace()
            .find_map(|field| field.strip_prefix("invocation="))
            .ok_or("inner line has no invocation label")?
            .to_owned();
        let edge = server
            .await_central_line(from, bound, |line| {
                line.contains(&format!("invocation={invocation} ")) && line.contains(" layer=edge ")
            })
            .await?;
        if !edge.ends_with("outcome=accepted owner") {
            return Err(format!("outer edge check logged {edge:?}").into());
        }
        let expected = if inner_outcome.contains("refused") {
            AccessVerificationOutcome::Refused(
                collaboration_central_core::AccessRefusal::BadSignature,
            )
        } else {
            AccessVerificationOutcome::Accepted(AccessCallerClass::Owner)
        };
        if answer != expected {
            return Err(format!("control answered {answer:?}").into());
        }
    }
    println!(
        "ACCESS log layers: outer owner accepted at the edge while the inner flipped signature is refused, inner unmodified accepted, one invocation label each: passed"
    );
    Ok(())
}

/// The random labels one object kind logged when its instances were built.
fn instance_labels(lines: &[String], kind: &str) -> std::collections::BTreeSet<String> {
    lines
        .iter()
        .filter(|line| line.starts_with(&format!("central object instance: kind={kind} ")))
        .filter_map(|line| line.split("instance=").nth(1).map(str::to_owned))
        .collect()
}
