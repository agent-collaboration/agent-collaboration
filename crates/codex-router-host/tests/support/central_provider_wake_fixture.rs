//! Actual central Wakes retain their Firing-to-local-Delivery correlation and ACP receipt.
use board_central_test_support::TestCentral;
use collaboration_central_conformance::{AutomationClockControl, ConformanceClient};
use collaboration_client::{CollaborationClient, FiringShowRequest, WakeCreateResponse};
use collaboration_protocol::{
    ApiSocketPath, DeliveryDisposition, DeliveryEvidence, DeliveryOutcome, DeliveryShowRequest,
    FiringOutcome, OperationId, SessionReachability, SessionRef,
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

async fn control(root: &Path, action: &str, request: Value) -> ProofResult<Value> {
    let response = reqwest::Client::builder()
        .unix_socket(root.join(ApiSocketPath::ServiceSocket.file_name()))
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?
        .post(format!("http://localhost/debug/automation/faults/{action}"))
        .json(&request)
        .send()
        .await?
        .error_for_status()?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        Ok(Value::Null)
    } else {
        Ok(response.json().await?)
    }
}

pub(super) async fn send_and_wait_wake(
    client: &CollaborationClient,
    central: &TestCentral,
    root: &Path,
    target: SessionRef,
    due: &str,
) -> ProofResult {
    let created = client.send_wakeup(serde_json::from_value(json!({
        "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":target,"content":{"kind":"humanUser","text":"wake through provider"},
            "delivery":"auto","generationGuard":null},
        "timing":{"kind":"at","at":due},"expiry":{"kind":"after","seconds":7200}
    }))?).await?;
    let WakeCreateResponse::Central { value: created } = created else {
        return Err("new provider Wake changed its central owner".into());
    };
    if created.first_firing.is_some() {
        return Err("controlled future Wake already had a Firing".into());
    }
    let arm = control(
        root,
        "arm",
        json!({
            "point":"reportSentBeforeAcknowledgement","selector":{"reportSeq":2}
        }),
    )
    .await?;
    ConformanceClient::new(central.base_url())?
        .automation_clock(AutomationClockControl::Advance {
            now: serde_json::from_value(json!(due))?,
        })
        .await?;
    // The existing control returns only after a real report answer. No timer or row polling.
    let held = match control(root, "wait", arm.clone()).await {
        Ok(held) => held,
        Err(error) => {
            let wake = client
                .read_wakeup(collaboration_protocol::WakeShowRequest {
                    wakeup_id: created.definition.wakeup_id.clone(),
                })
                .await;
            let firings = client
                .list_firings(collaboration_protocol::FiringListRequest {
                    automation_id: created
                        .definition
                        .wakeup_id
                        .as_str()
                        .to_owned()
                        .try_into()?,
                    cursor: None,
                    limit: 25.try_into()?,
                })
                .await;
            use sqlx::Connection as _;
            let mut sql = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
            )
            .await?;
            let local: Vec<String> = sqlx::query_scalar("SELECT state FROM firing_receipts")
                .fetch_all(&mut sql)
                .await?;
            let owed: Vec<String> =
                sqlx::query_scalar("SELECT report_json FROM central_report_outbox")
                    .fetch_all(&mut sql)
                    .await?;
            sql.close().await?;
            return Err(format!("report hold failed: {error}; Wake={wake:?}; Firings={firings:?}; local={local:?}; owed={owed:?}").into());
        }
    };
    let proof: ProofResult = async {
        if arm.get("armId").is_none()
            || held.get("armId") != arm.get("armId")
            || held.get("point") != Some(&json!("reportSentBeforeAcknowledgement"))
            || held.pointer("/evidence/reportSeq") != Some(&json!(2))
        {
            return Err("provider Wake reached another causal hold".into());
        }
        let firing_id = serde_json::from_value(
            held.pointer("/evidence/firingId")
                .ok_or("held report missing Firing identity")?
                .clone(),
        )?;
        let firing = client.read_firing(FiringShowRequest { firing_id }).await?;
        if firing.automation_id.as_str() != created.definition.wakeup_id.as_str()
            || !matches!(firing.outcome, FiringOutcome::Delivered)
        {
            return Err(format!("provider Wake report was not Delivered: {firing:?}").into());
        }
        let mut store =
            automation_storage::AutomationStore::open(&root.join("automation.sqlite")).await?;
        let receipt = store.read_central_firing(&firing.firing_id).await?;
        store.close().await?;
        let Some(receipt) = receipt else {
            return Err("real local Firing receipt missing".into());
        };
        let automation_storage::CentralLocalWork::Wake { delivery_id } = receipt.local_work else {
            return Err("central Wake lost its actual local Delivery identity".into());
        };
        let delivery = client
            .read_delivery(DeliveryShowRequest { delivery_id })
            .await?;
        if !matches!(delivery.disposition, DeliveryDisposition::Accepted)
            || !matches!(delivery.evidence, DeliveryEvidence::Accepted { receipt, .. }
                if receipt.outcome == DeliveryOutcome::Started
                    && receipt.reachability == Some(SessionReachability::ProviderAcp))
        {
            return Err("central Wake lost the original Started ACP receipt".into());
        }
        Ok(())
    }
    .await;
    let released = control(root, "release", arm).await;
    proof?;
    released?;
    Ok(())
}
