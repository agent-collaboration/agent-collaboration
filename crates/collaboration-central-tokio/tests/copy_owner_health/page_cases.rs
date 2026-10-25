//! Page crosses HTTP and reads the canonical Fleet directory after store reopen.
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::{
    copy_owner_scenarios::OwnerClient,
    fleet_setup::{Fixture, bundle},
};
use fleet_admission::DirectoryEntry;
use serde_json::{Value, json};

#[tokio::test]
async fn owner_page_orders_and_bounds_real_directory_entries_and_continues_exclusively()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let mut imported = bundle()?;
    // Deliberately unsorted input. The existing imported project sorts first.
    for (suffix, title) in [("4743", "Third"), ("4741", "First"), ("4742", "Second")] {
        imported.records.push(serde_json::from_value(json!({
            "kind":"directory", "project":format!("019f0000-0000-7000-8000-00000000{suffix}"),
            "board":"019f0000-0000-7000-8000-000000000a82", "title":title,
            "time":{"kind":"legacyUnknown"}
        }))?);
    }
    let arranged = fixture.import(&imported).await;
    fixture.objects.close().await?;
    arranged?;
    drop(fixture);
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let client = OwnerClient::new(server.base_url())?;
        let expected = [
            "019f0000-0000-7000-8000-000000000a83",
            "019f0000-0000-7000-8000-000000004741",
            "019f0000-0000-7000-8000-000000004742",
            "019f0000-0000-7000-8000-000000004743",
        ];
        let mut after = None;
        let mut observed = Vec::new();
        for _ in 0..3 {
            let route = match &after {
                Some(after) => format!("/admin/v1/copies?limit=2&after={after}"),
                None => "/admin/v1/copies?limit=2".into(),
            };
            let (status, page) = client.get(&route).await?;
            if status != reqwest::StatusCode::OK {
                return Err(format!("populated Page failed: {status}: {page}").into());
            }
            let entries: Vec<DirectoryEntry> = serde_json::from_value(
                page.get("projects")
                    .cloned()
                    .ok_or("missing directory projects")?,
            )?;
            let expected_count = if observed.len() == 4 { 0 } else { 2 };
            if entries.len() != expected_count {
                return Err("Page ignored its actual bound or exclusive cursor".into());
            }
            for entry in &entries {
                if entry.workspace.as_str() != "019f0000-0000-7000-8000-000000000a81"
                    || entry.board.as_str() != "019f0000-0000-7000-8000-000000000a82"
                    || !matches!(
                        entry.copy_side,
                        fleet_admission::DirectorySide::NotYetReported
                    )
                    || !matches!(
                        entry.lease_side,
                        fleet_admission::DirectorySide::NotYetReported
                    )
                {
                    return Err(
                        "Page lost the canonical directory join or fabricated a side projection"
                            .into(),
                    );
                }
                observed.push(entry.project.as_str().to_owned());
            }
            after = page
                .get("nextAfter")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if after.as_deref() != entries.last().map(|entry| entry.project.as_str()) {
                return Err("Page cursor does not name its last actual project".into());
            }
            let as_of = page
                .get("asOf")
                .and_then(Value::as_str)
                .ok_or("missing Page asOf")?;
            let _: message_board::RecordTime =
                serde_json::from_value(json!({"kind":"router","value":as_of}))?;
        }
        if observed != expected {
            return Err(
                format!("Page sorted or skipped identities incorrectly: {observed:?}").into(),
            );
        }
        let (status, page) = client.get("/admin/v1/copies?limit=100").await?;
        if status != reqwest::StatusCode::OK
            || page.get("projects").and_then(Value::as_array).map(Vec::len) != Some(4)
        {
            return Err("Page did not admit its inclusive maximum limit".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
