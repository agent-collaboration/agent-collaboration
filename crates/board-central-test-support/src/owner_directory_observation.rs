//! Observe Directory through its real owner HTTP route, without opening live stores.
use crate::{BOUND, FixtureResult, TestCentral};
use collaboration_central_conformance::owner_assertion;
use message_board::ProjectId;
use serde_json::{Value, json};

impl TestCentral {
    pub async fn copy_status(
        &self,
        project: &ProjectId,
    ) -> FixtureResult<fleet_sync_protocol::CopyStatus> {
        use collaboration_central_admin_protocol::{CopyHealth, CopyHealthRead};

        let health: CopyHealth = reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .get(format!(
                "{}/admin/v1/copies/{}/health",
                self.base_url(),
                project.as_str()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        match health {
            CopyHealth::Live(health) => match health.copy {
                CopyHealthRead::Known(Some(status)) => Ok(status),
                CopyHealthRead::Known(None) => Err("Copy health has no native status".into()),
                CopyHealthRead::Unreadable { detail } => {
                    Err(format!("Copy native status is unreadable: {detail}").into())
                }
            },
            CopyHealth::Retired { .. } => Err("Copy health is retired".into()),
        }
    }

    pub async fn takeover_presence(&self, project: &ProjectId) -> FixtureResult<Value> {
        Ok(reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .post(format!(
                "{}/admin/v1/projects/{}/takeover/preview",
                self.base_url(),
                project.as_str()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&Value::Null)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn directory_entry(&self, project: &ProjectId) -> FixtureResult<Value> {
        let page: Value = reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .get(format!("{}/admin/v1/copies?limit=100", self.base_url()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let projects = page
            .get("projects")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("Directory omitted projects: {page}"))?;
        let matching = projects
            .iter()
            .filter(|entry| entry.get("project") == Some(&json!(project)))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!(
                "expected one Directory entry for {}: {page}",
                project.as_str()
            )
            .into());
        }
        Ok((*matching.first().ok_or("Directory project absent")?).clone())
    }
}
