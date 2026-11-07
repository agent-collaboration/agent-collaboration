use super::{E2eResult, promote_error, promote_result};
use crate::upload_support;
use serde_json::Value;
use std::path::Path;

pub fn required_readback_cell(row: &Value, column: usize) -> E2eResult<&Value> {
    row.get(column)
        .ok_or_else(|| promote_error(format!("readback column {column} missing")))
}

pub async fn project_state_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(project_id,description,workspace_id,board_id,epoch,position,router_time,time_basis) AS value FROM project_state",
    )
    .await
}

pub async fn feed_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(epoch,position,record_kind,record_id,router_time,time_basis) AS value FROM project_feed ORDER BY position",
    )
    .await
}

pub async fn post_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(p.post_id,p.thread_id,p.placement_kind,json(actor.canonical_identity),json(p.acting_for),json(p.posted_as_role),p.position) AS value FROM posts p JOIN actors actor ON actor.actor_id=p.actor_id ORDER BY p.position",
    )
    .await
}

pub async fn mark_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(json(reader.canonical_identity),scope_kind,scope_id,through_epoch,through_position) AS value FROM read_marks JOIN actors reader ON reader.actor_id=reader_actor ORDER BY reader.canonical_identity,scope_kind,scope_id",
    )
    .await
}

pub async fn thread_watch_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(json(reader.canonical_identity),thread_id,start_epoch,start_position,json(CASE WHEN active=1 THEN 'true' ELSE 'false' END)) AS value FROM thread_watches JOIN actors reader ON reader.actor_id=reader_actor ORDER BY reader.canonical_identity,thread_id",
    )
    .await
}

pub async fn project_watch_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(json(reader.canonical_identity),start_epoch,start_position,json(CASE WHEN active=1 THEN 'true' ELSE 'false' END)) AS value FROM project_watches JOIN actors reader ON reader.actor_id=reader_actor ORDER BY reader.canonical_identity",
    )
    .await
}

pub async fn thread_participation_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(json(actor.canonical_identity),role,joined_epoch,joined_position,last_seen_epoch,last_seen_position,closed_epoch,closed_position,closed_reason,json(replaced.canonical_identity)) AS value FROM thread_participation JOIN actors actor ON actor.actor_id=thread_participation.actor_id LEFT JOIN actors replaced ON replaced.actor_id=thread_participation.replaced_by WHERE thread_id='019a0000-0000-7000-8000-000000000065' ORDER BY joined_position,actor.canonical_identity",
    )
    .await
}

pub async fn membership_rows(root: &Path, project: &str) -> E2eResult<Vec<Value>> {
    rows(
        root,
        project,
        "SELECT json_array(json(reader.canonical_identity),scope_kind,scope_id,policy,renewed_at,expires_at,generation,activated_epoch,activated_position,ended) AS value FROM subscription_memberships JOIN actors reader ON reader.actor_id=reader_actor ORDER BY reader.canonical_identity,scope_kind,scope_id,generation",
    )
    .await
}

pub async fn published_snapshot(root: &Path, project: &str) -> E2eResult<Vec<String>> {
    promote_result(
        upload_support::read(
            root,
            &format!("copy-{project}"),
            upload_support::COPY_SNAPSHOT,
        )
        .await,
    )
}

async fn rows(root: &Path, project: &str, query: &'static str) -> E2eResult<Vec<Value>> {
    promote_result(upload_support::read(root, &format!("copy-{project}"), query).await)?
        .into_iter()
        .map(|row| serde_json::from_str(&row).map_err(promote_error))
        .collect()
}
