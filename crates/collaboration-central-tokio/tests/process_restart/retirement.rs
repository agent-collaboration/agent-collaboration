//! Process death between Registry's retirement admission and Copy wipe.
use super::*;
use fleet_sync_protocol::DatabaseId;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn retirement_resumes_its_stored_step_after_the_server_process_dies() -> ConformanceResult<()>
{
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?;
    arrange(root.path(), database).await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    control(
        &first.base,
        ObjectAddress::Registry,
        RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Copy(database),
        },
    )
    .await?;
    let route = format!("/admin/v1/projects/{database}/retire");
    let request =
        json!({"requestId":Uuid::now_v7(),"expectFence":1,"expectAcked":{"epoch":1,"sequence":1}});
    let base = first.base.clone();
    let pending_route = route.clone();
    let pending_request = request.clone();
    let pending = tokio::spawn(async move { post(&base, &pending_route, &pending_request).await });
    let held = control(
        &first.base,
        ObjectAddress::Registry,
        RuntimeControl::AwaitHeldDispatch,
    )
    .await?;
    let DispatchFaultObservation::Held { request: bytes, .. } = held.dispatch else {
        return Err("retirement did not reach its Copy barrier".into());
    };
    let call: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(call.get("kind"), Some(&json!("fleet-sync.copy-retire")));
    assert_eq!(call.pointer("/identity/step"), Some(&json!("retire_copy")));
    let identity = call
        .pointer("/identity/operation")
        .ok_or("retirement omitted its durable operation")?
        .clone();
    let registry_path = root.path().join("registry.db");
    let copy_path = root.path().join(format!("copy-{database}.db"));
    let before = operation(&registry_path, &identity).await?;
    assert_eq!(before.0, "running");
    assert_eq!(
        before.2, 4,
        "check, admission, seal and pending wipe must be durable"
    );
    assert_eq!(
        read_count(&copy_path, "SELECT COUNT(*) AS count FROM claim_rows").await?,
        1
    );
    first.crash().await?;
    let _ = pending.await?;

    let second = ServerProcess::start(root.path(), &jwks.url).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    // Reading Fleet's owner directory never drives the Registry journal.
    // Observe convergence before replaying the interrupted retirement request.
    tokio::time::timeout(RECOVERY_BOUND, async {
        loop {
            let page = directory(&http, &second.base).await?;
            let projected = page
                .get("projects")
                .and_then(Value::as_array)
                .and_then(|projects| {
                    projects
                        .iter()
                        .find(|project| project.get("project") == Some(&json!(database)))
                })
                .ok_or("restart directory lost the imported project")?;
            if projected.pointer("/leaseSide/value/lease/kind") == Some(&json!("retired")) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "startup did not finish the interrupted retirement")??;
    let expected = post(&second.base, &route, &request).await?;
    assert_eq!(expected.get("kind"), Some(&json!("retired")));
    assert_eq!(post(&second.base, &route, &request).await?, expected);
    second.stop().await?;
    let after = operation(&registry_path, &identity).await?;
    assert_eq!(after.0, "completed");
    assert_eq!(
        read_count(
            &registry_path,
            "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='fleet-sync.retire'"
        )
        .await?,
        1
    );
    assert_eq!(
        read_count(&copy_path, "SELECT COUNT(*) AS count FROM claim_rows").await?,
        0
    );
    let result: Value =
        serde_json::from_slice(after.1.as_ref().ok_or("retirement result missing")?)?;
    assert_eq!(result, expected);
    // A fresh Registry run must acknowledge its own Directory publication before
    // exposing Retired again. Hold the existing incarnation boundary so this
    // valid Pending(Directory) response cannot race startup's publication.
    let controls = [collaboration_central_conformance::RuntimeControlRequest {
        address: ObjectAddress::Registry,
        command: RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Fleet,
        },
    }];
    let third = ServerProcess::start_with_controls(root.path(), &jwks.url, &controls).await?;
    let replay = async {
        let held = control(
            &third.base,
            ObjectAddress::Registry,
            RuntimeControl::AwaitHeldDispatch,
        )
        .await?;
        let DispatchFaultObservation::Held {
            destination,
            request: bytes,
        } = held.dispatch
        else {
            return Err("restart missed its Registry incarnation barrier".into());
        };
        let call: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(destination, ObjectAddress::Fleet);
        assert_eq!(call.get("kind"), Some(&json!("central.lease-incarnation")));
        assert_eq!(
            post(&third.base, &route, &request).await?,
            json!({"kind":"pending","value":{"requestId":request.get("requestId"),"phase":"directory"}})
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let released = control(
        &third.base,
        ObjectAddress::Registry,
        RuntimeControl::ResumeDispatch,
    )
    .await;
    let stopped = third.stop().await;
    replay?;
    released?;
    stopped?;
    assert_eq!(
        operation(&registry_path, &identity).await?,
        after,
        "replay or another startup repeated a recorded step"
    );
    println!(
        "RETIREMENT_RESTART_STAGE fresh_publication_gate_and_original_completed_journal passed"
    );
    jwks.shutdown().await?;
    Ok(())
}

async fn arrange(root: &std::path::Path, database: DatabaseId) -> ConformanceResult<()> {
    use fleet_admission::{FleetImportRecord, FleetImportResult, ImportedBoardState};
    // Reuse the native retirement fixture's canonical graph, public ImportTask
    // and real seeded Copy. HTTP import orchestration is outside this proof.
    let fleet = native_fleet::Fixture::open(root).await?;
    let mut bundle = native_fleet::bundle()?;
    for record in &mut bundle.records {
        if let FleetImportRecord::Board { state, .. } = record {
            *state = ImportedBoardState::Archived;
        }
    }
    let imported = fleet.import(&bundle).await;
    fleet.objects.close().await?;
    assert!(matches!(imported?, FleetImportResult::Imported { .. }));
    registry_setup::import_seeded(root, configuration::registry_config()?, database).await
}
