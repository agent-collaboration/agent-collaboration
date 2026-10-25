//! Abrupt loss on either side of native admission replays the original wake exactly once.
use super::*;
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};

#[tokio::test]
async fn owed_wake_process_loss_replays_before_admission_and_before_acknowledgement()
-> TestResult<()> {
    for stage in ["fire-owed", "push-admitted-before-ack"] {
        let fixture_id = OperationId::generate();
        let root = native_delivery_crash_root(&fixture_id);
        std::fs::DirBuilder::new().mode(0o700).create(&root)?;
        let child = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "wakeup_delivery_sender::crash_tests::native_delivery_crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env(ROOT_ENV, &root)
                .env(STAGE_ENV, stage)
                .env(ID_ENV, fixture_id.as_str())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        if child.status.code() != Some(CRASH_EXIT) {
            return Err(format!(
                "{stage}: child did not reach handoff checkpoint: {:?}; {}; {}",
                child.status.code(),
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            )
            .into());
        }
        if root.join("native-receipt.json").exists() {
            return Err("handoff crash dispatched native input before restarting".into());
        }
        let delivery_id: DeliveryId =
            serde_json::from_slice(&std::fs::read(root.join("delivery-id.json"))?)?;
        let push_id: PushId = serde_json::from_slice(&std::fs::read(root.join("push-id.json"))?)?;
        let automation = Arc::new(Mutex::new(
            AutomationStore::open(&root.join("automation.sqlite")).await?,
        ));
        let identity = identity(&root, Arc::clone(&automation), NEW_EPOCH).await?;
        let interactions = identity
            .interaction_store
            .clone()
            .ok_or("interaction owner missing")?;
        let owed = automation
            .lock()
            .await
            .list_owed_push_operations(100)
            .await?;
        if owed.len() != 1
            || owed[0].push_id != push_id
            || interactions.get_push_record(&push_id).await?.is_some()
                != (stage == "push-admitted-before-ack")
        {
            return Err(
                "crash checkpoint did not retain the exact owed/native admission boundary".into(),
            );
        }
        let WakeAcceptanceFixture {
            native,
            correlation,
        } = accepting_native_call(&root)?;
        let stop = CancellationToken::new();
        let handoff = tokio::spawn(
            identity
                .push_handoff_worker()
                .ok_or("handoff worker missing")?
                .run(stop.clone()),
        );
        let wake = tokio::spawn(
            identity
                .wake_timing_worker()
                .ok_or("wake worker missing")?
                .run(stop.clone()),
        );
        let observed = tokio::time::timeout(Duration::from_secs(5), async {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                let delivery = automation.lock().await.read_delivery::<
                    SessionRef, CodexGeneration, crate::stored_delivery_receipt::StoredDeliveryReceipt,
                >(&delivery_id).await?;
                if delivery.status == agent_automation::DeliveryStatus::Accepted {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(delivery);
                }
            }
        }).await;
        stop.cancel();
        handoff.await?;
        wake.await?;
        let delivery = observed??;
        if tokio::time::timeout(Duration::from_secs(5), correlation).await?? != push_id.as_str() {
            return Err("restarted wake used a different native input identity".into());
        }
        native.await??;
        if delivery
            .attempt
            .as_ref()
            .is_none_or(|attempt| attempt.attempt_number != 1)
            || !automation
                .lock()
                .await
                .list_owed_push_operations(100)
                .await?
                .is_empty()
            || !automation
                .lock()
                .await
                .eligible_delivery_ids(chrono::Utc::now().timestamp_millis() + 60_000, 100)
                .await?
                .is_empty()
            || interactions
                .get_push_record(&push_id)
                .await?
                .is_none_or(|push| {
                    push.delivery_state != collaboration_protocol::PushDeliveryState::Delivered
                })
        {
            return Err(
                "handoff replay lost, duplicated or left the wake eligible for another attempt"
                    .into(),
            );
        }
        let mut observer = TursoConnection::connect_with(
            &TursoConnectOptions::new().filename(root.join("interaction.sqlite")),
        )
        .await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM router_pushes")
            .fetch_one(&mut observer)
            .await?;
        if count != 1 {
            return Err("handoff replay duplicated the physical native push".into());
        }
        observer.close().await?;
        drop(identity);
        Arc::try_unwrap(interactions)
            .map_err(|_| "handoff crash interaction owner still shared")?
            .close()
            .await?;
        Arc::try_unwrap(automation)
            .map_err(|_| "handoff crash automation owner still shared")?
            .into_inner()
            .close()
            .await?;
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                return Err("unexpected handoff fixture directory".into());
            }
            std::fs::remove_file(entry.path())?;
        }
        std::fs::remove_dir(root)?;
    }
    Ok(())
}
