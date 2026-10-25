//! Owner HTTP operations remain live while Router sessions serve their calls.
use crate::{BOUND, FixtureResult, TestCentral};
use collaboration_central_conformance::owner_assertion;
use fleet_sync_protocol::{DatabaseId, TransferId};
use message_board::ServiceId;
use serde_json::{Value, json};

impl TestCentral {
    pub async fn move_project(
        &self,
        database: DatabaseId,
        target: &ServiceId,
        transfer: TransferId,
    ) -> FixtureResult<Value> {
        self.finish_owner_transfer(database, "move", json!({"transferId":transfer,"toInstallation":target,"reason":"real two-Router move proof"})).await
    }

    pub async fn take_over_project(
        &self,
        database: DatabaseId,
        target: &ServiceId,
        transfer: TransferId,
    ) -> FixtureResult<Value> {
        let response = self
            .owner_transfer_post(database, "takeover/preview", &Value::Null)
            .await?;
        let preview: fleet_sync_protocol::TakeoverPreview = serde_json::from_value(
            response
                .get("preview")
                .cloned()
                .ok_or("owner response omitted native preview")?,
        )?;
        self.finish_owner_transfer(database, "takeover", json!({"transferId":transfer,"toInstallation":target,"reason":"real two-Router takeover proof","preview":preview})).await
    }

    async fn finish_owner_transfer(
        &self,
        database: DatabaseId,
        operation: &str,
        body: Value,
    ) -> FixtureResult<Value> {
        tokio::time::timeout(BOUND, async {
            loop {
                let answer = self.owner_transfer_post(database, operation, &body).await?;
                if answer.get("kind") != Some(&json!("pending")) {
                    return Ok(answer);
                }
                tokio::task::yield_now().await;
            }
        })
        .await?
    }

    async fn owner_transfer_post(
        &self,
        database: DatabaseId,
        operation: &str,
        body: &Value,
    ) -> FixtureResult<Value> {
        Ok(reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .post(format!(
                "{}/admin/v1/projects/{database}/{operation}",
                self.server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
