//! The real library holder change commits its view and both ownership streams together.
use super::composed_fleet_fixture::*;
use collaboration_central_core::ObjectPlatform;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_protocol::{ClientFrame, ConnectionId, SchemaSupport, SessionId};
use fleet_session::{ConnectionBinding, VerifiedPeer};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{collections::BTreeSet, sync::atomic::Ordering};

async fn takeover_input(fixture: &Fixture) -> TestResult<Value> {
    let preview = fixture.owner(json!({"kind":"preview"})).await?;
    let preview = preview
        .pointer("/value/value/preview")
        .cloned()
        .ok_or_else(|| format!("real preview missing: {preview}"))?;
    Ok(
        json!({"kind":"takeover","value":{"transferId":durable_steps::Uuid::now_v7(),"toInstallation":SERVICE_B,"reason":"atomic ownership proof","preview":preview}}),
    )
}
async fn task_states(fixture: &Fixture, state: &'static str) -> TestResult<BTreeSet<String>> {
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT kind FROM _fleet_sync_operations WHERE state=? ORDER BY kind",
                )
                .bind(state)
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| row.try_get("kind").map_err(StoreError::backend))
                .collect::<Result<_, _>>()
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions independently compare the before and after lease, view and both ownership streams."
)]
#[tokio::test]
async fn the_second_holder_notice_failure_rolls_back_lease_directory_counter_and_the_first_notice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let before = fixture.lease_and_notices().await?;
    assert_eq!(
        before.3,
        vec![(A.into(), "lease_granted".into(), "1".into())]
    );
    let request = takeover_input(&fixture).await?;
    fixture.sql("CREATE TRIGGER reject_new_holder_notice BEFORE INSERT ON machine_control_project_notices WHEN NEW.kind='lease_granted' AND NEW.epoch=2 BEGIN SELECT RAISE(ABORT,'injected second holder notice failure'); END").await?;
    let pending = fixture.owner(request.clone()).await?;
    assert_eq!(
        pending.pointer("/value/kind"),
        Some(&json!("pending")),
        "{pending}"
    );
    assert_eq!(
        fixture.lease_and_notices().await?,
        before,
        "the failed second notice must roll back the first notice and all lease facts"
    );
    fixture.sql("DROP TRIGGER reject_new_holder_notice").await?;
    fixture.io.now.store(NOW + 10_000, Ordering::SeqCst);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let answer = fixture.owner(request.clone()).await?;
    assert_eq!(
        answer.pointer("/value/kind"),
        Some(&json!("granted")),
        "{answer}"
    );
    let committed = fixture.lease_and_notices().await?;
    assert_eq!(committed.0.pointer("/grant/holder"), Some(&json!(B)));
    assert_eq!(committed.0.pointer("/grant/epoch"), Some(&json!(2)));
    assert_eq!(committed.2, 2);
    assert_eq!(
        committed.3,
        vec![
            (A.into(), "lease_granted".into(), "1".into()),
            (A.into(), "lease_superseded".into(), "1".into()),
            (B.into(), "lease_granted".into(), "2".into())
        ]
    );
    let projection: Value =
        serde_json::from_str(committed.1.as_deref().ok_or("directory view absent")?)?;
    assert_eq!(projection.get("snapshot"), Some(&committed.0));
    assert_eq!(fixture.owner(request).await?, answer);
    assert_eq!(
        fixture.lease_and_notices().await?,
        committed,
        "replay must add no notice or incarnation"
    );
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The restart must execute real mixed workflow kinds through the single production Fleet engine."
)]
#[tokio::test]
async fn one_fleet_dispatcher_recovers_holder_change_feed_notice_and_schema_support_after_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let request = takeover_input(&fixture).await?;
    fixture.sql("CREATE TRIGGER reject_new_holder_notice BEFORE INSERT ON machine_control_project_notices WHEN NEW.kind='lease_granted' AND NEW.epoch=2 BEGIN SELECT RAISE(ABORT,'injected mixed restart boundary'); END").await?;
    fixture.owner(request.clone()).await?;
    fixture.sql("DROP TRIGGER reject_new_holder_notice").await?;
    // Register an actual Fleet reader, then a metadata write owes it a feed notice.
    fixture
        .call(
            "session",
            "central.fleet-read",
            Value::Null,
            json!({"kind":"feed","value":{"after":0}}),
        )
        .await?;
    fixture
        .metadata(fleet_admission::FleetOperation::WorkspaceUpdate {
            name: "updated workspace".to_owned().try_into()?,
            description: "mixed journal restart".to_owned().try_into()?,
        })
        .await?;
    // A real Hello admits the library's schema workflow; no alarm runs before close.
    let binding = ConnectionBinding::new(
        ConnectionId::new(durable_steps::Uuid::now_v7())?,
        VerifiedPeer {
            peer: Fixture::peer()?,
            expires_at: fleet_protocol::SyncTime::from_unix_millis(NOW + 60_000),
        },
    );
    let mut socket = fixture
        .host
        .attach_socket(
            binding,
            collaboration_central_core::AdmissionOrigin {
                installation: SERVICE_A.to_owned().try_into()?,
                machine: "composed fixture".to_owned().try_into()?,
            },
        )
        .await;
    fixture.runtime.on_socket_opened(binding).await?;
    fixture
        .runtime
        .on_frame(
            binding,
            serde_json::to_vec(
                &ClientFrame::<board_sync_protocol::BoardConsumerProtocol>::Hello {
                    capabilities: None,
                    session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
                    schema_support: SchemaSupport { max_version: 7 },
                },
            )?,
        )
        .await?;
    assert!(
        socket.try_recv().is_err(),
        "a Hello must not produce Welcome before its schema task commits"
    );
    let before = task_states(&fixture, "running").await?;
    assert!(before.contains("fleet-sync.holder-change"), "{before:?}");
    assert!(before.contains("central.fleet-notice"), "{before:?}");
    assert!(
        before.contains("fleet-sync.publish-schema-support"),
        "{before:?}"
    );
    drop(socket);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 10_000).await?;
    assert!(reopened.host.registrations().await?.is_empty());
    // Every call is to the production engine; injected time advances only saved retry deadlines.
    for offset in 1..=5 {
        reopened
            .io
            .now
            .store(NOW + offset * 10_000, Ordering::SeqCst);
        crate::kernel_alarm::deliver_alarm(&reopened.runtime).await?;
    }
    assert!(
        task_states(&reopened, "failed").await?.is_empty(),
        "unknown or wrong task registration failed after reopen"
    );
    let completed = task_states(&reopened, "completed").await?;
    for kind in [
        "fleet-sync.holder-change",
        "central.fleet-notice",
        "fleet-sync.publish-schema-support",
        "central.machine-notice-append",
    ] {
        assert!(
            completed.contains(kind),
            "{kind} missing from {completed:?}"
        );
    }
    assert_eq!(
        reopened.owner(request).await?.pointer("/value/kind"),
        Some(&json!("granted"))
    );
    let committed = reopened.lease_and_notices().await?;
    assert_eq!(committed.0.pointer("/grant/holder"), Some(&json!(B)));
    assert_eq!(
        committed
            .3
            .iter()
            .filter(|(_, kind, _)| kind == "lease_superseded")
            .count(),
        1
    );
    reopened.close().await?;
    Ok(())
}
