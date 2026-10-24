//! Presence and renewal are independent real Registry observations (decision586).
use super::*;
use collaboration_central_conformance::RuntimeControl;

#[tokio::test]
async fn owner_preview_keeps_presence_distinct_from_the_native_renewal_observation()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof = async {
        fixture
            .control(RuntimeControl::SetTime {
                now: serde_json::from_value(json!("2030-03-17T17:46:40.000Z"))?,
            })
            .await?;
        fixture.renew_old().await?;
        fixture
            .control(RuntimeControl::SetTime {
                now: serde_json::from_value(json!("2030-03-17T17:47:40.000Z"))?,
            })
            .await?;
        fixture.refresh_old_presence().await?;
        let body = fixture.post("takeover/preview", &Value::Null).await?;
        let native: TakeoverPreview =
            serde_json::from_value(body.get("preview").cloned().ok_or_else(|| {
                format!("preview success lacks separate native binding and presence: {body}")
            })?)?;
        if native.last_seen != Some(SyncTime::from_unix_millis(1_900_000_000_000)) {
            return Err(
                "preview changed the native renewal observation used by confirmation".into(),
            );
        }
        if body.get("holderInstallation") != Some(&json!(SERVICE))
            || body.get("lastSeen") != Some(&json!(SyncTime::from_unix_millis(1_900_000_060_000)))
        {
            return Err(format!(
                "preview reported renewal as presence or confused peer with installation: {body}"
            )
            .into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.close().await?;
    proof
}
