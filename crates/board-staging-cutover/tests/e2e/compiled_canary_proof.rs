//! Independent public readback must precede the fixture's own replay or it could hide a missing CLI write.
use super::{
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
};
use board_staging_cutover::PreparedProjectBundle;
use collaboration_client::CollaborationClient;
use message_board::{
    FeedCursor, FeedRead, FeedReadRequest, PageLimit, ReadActor, WorkTrailReadResult,
    WorkTrailSelection, WorkTrailShowRequest,
};

pub(super) async fn verify(
    client: &CollaborationClient,
    bundle: &PreparedProjectBundle,
    run: &RunFixture,
    head: FeedCursor,
) -> TestResult {
    let cursor = FeedCursor {
        epoch: head.epoch,
        position: head
            .position
            .get()
            .checked_add(1)
            .ok_or("canary cursor overflow")?
            .try_into()?,
    };
    let after = FeedCursor {
        epoch: head.epoch,
        position: head
            .position
            .get()
            .checked_add(2)
            .ok_or("canary head overflow")?
            .try_into()?,
    };
    for (disposition, before) in [("created", head), ("replayed", after)] {
        let result = run.execute_canary().await?;
        require(
            result.status.success(),
            &format!(
                "compiled canary failed: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ),
        )?;
        let value: serde_json::Value = serde_json::from_slice(&result.stdout)?;
        let projects = value
            .get("projects")
            .and_then(serde_json::Value::as_array)
            .ok_or("canary project report missing")?;
        let [project] = projects.as_slice() else {
            return Err("one canary report required".into());
        };
        require(
            value.get("kind").and_then(serde_json::Value::as_str) == Some("canariesVerified")
                && value.get("runId") == Some(&serde_json::to_value(bundle.manifest().run_id)?)
                && project.get("kind").and_then(serde_json::Value::as_str) == Some("verified")
                && project
                    .get("disposition")
                    .and_then(serde_json::Value::as_str)
                    == Some(disposition)
                && project.get("project")
                    == Some(&serde_json::to_value(
                        &bundle.manifest().source.selected.project,
                    )?)
                && project.get("request")
                    == Some(&serde_json::to_value(&bundle.manifest().canary_request)?)
                && project.get("cursor") == Some(&serde_json::to_value(cursor)?)
                && project.get("headBefore") == Some(&serde_json::to_value(before)?)
                && project.get("headAfter") == Some(&serde_json::to_value(after)?),
            "compiled canary report changed its sealed identity or recovered cursor",
        )?;
        let read = client
            .work_trail_show(WorkTrailShowRequest {
                reader: ReadActor::Anonymous {},
                project_id: bundle.manifest().source.selected.project.clone(),
                selection: WorkTrailSelection::Forward { after: head },
                limit: PageLimit::try_from(10)?,
                subject: None,
                tags: vec![],
                include_redacted: true,
            })
            .await?;
        let WorkTrailReadResult::Forward(page) = read.value else {
            return Err("compiled canary lost its recovered cursor".into());
        };
        let [row] = page.rows.as_slice() else {
            return Err("compiled canary did not create exactly one real public record".into());
        };
        require(
            row.row_id.as_str() == bundle.manifest().canary_request.as_str()
                && row.text.as_str() == format!("C5b cutover canary {}", bundle.manifest().run_id)
                && row.envelope.request.as_ref() == Some(&bundle.manifest().canary_request)
                && row.envelope.position.project_cursor() == Some(cursor),
            "compiled canary public body or cursor differs",
        )?;
        let FeedRead::Entries { high_water, .. } = client
            .feed_read(FeedReadRequest {
                reader: ReadActor::Anonymous {},
                project_id: bundle.manifest().source.selected.project.clone(),
                after,
                limit: PageLimit::try_from(1)?,
                include_redacted: true,
            })
            .await?
            .value
        else {
            return Err("compiled canary head disappeared".into());
        };
        require(
            high_water == after,
            "compiled canary replay appended extra application history",
        )?;
    }
    Ok(())
}
