//! Checked SQL borrows A's one writer; conflicts never overwrite destination data.
use crate::{
    message_copy::MessageCopyHost,
    message_rows::{IntentCursorRow, IntentRow, LegacyPushRow, MessageRows, SubscriptionRow},
};
use durable_steps::{StoreError, storage::TransactionScope, turso_store::TursoScope};

pub(crate) async fn insert(
    scope: &mut TursoScope<'_, MessageCopyHost>,
    offered: &MessageRows,
) -> Result<(), StoreError> {
    for row in &offered.pushes {
        sqlx_turso::query!("INSERT INTO router_pushes(push_id,kind,origin_kind,origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id,target_endpoint_id,target_session_id,dm_delivery_mode,dm_generation_guard_json,reply_to_push_id,header_facts_json,body,ranges_json,delivery_state,last_outcome_json,created_at,settled_at,read_at,retention) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'creationAge') ON CONFLICT DO NOTHING", &row.push_id, &row.kind, &row.origin_kind, &row.origin_service_id, &row.origin_endpoint_id, &row.origin_session_id, &row.origin_router_ref, &row.target_service_id, &row.target_endpoint_id, &row.target_session_id, &row.dm_delivery_mode, &row.dm_generation_guard_json, &row.reply_to_push_id, &row.header_facts_json, &row.body, &row.ranges_json, &row.delivery_state, &row.last_outcome_json, &row.created_at, &row.settled_at, &row.read_at).execute(scope.executor()).await.map_err(StoreError::backend)?;
    }
    for row in &offered.subscriptions {
        let progress_id = uuid::Uuid::now_v7().to_string();
        sqlx_turso::query!("INSERT INTO subscription_delivery_progress(progress_id,project_id,membership_id,activated_epoch,activated_position,payload) VALUES (?,?,?,?,?,?) ON CONFLICT DO NOTHING", progress_id, &row.project_id, &row.membership_id, &row.activated_epoch, &row.activated_position, &row.payload).execute(scope.executor()).await.map_err(StoreError::backend)?;
    }
    for row in &offered.intents {
        let progress_id = uuid::Uuid::now_v7().to_string();
        sqlx_turso::query!("INSERT INTO board_intent_progress(progress_id,project_id,intent_id,state_epoch,state_position) VALUES (?,?,?,?,?) ON CONFLICT DO NOTHING", progress_id, &row.project_id, &row.intent_id, &row.state_epoch, &row.state_position).execute(scope.executor()).await.map_err(StoreError::backend)?;
    }
    for row in &offered.cursors {
        sqlx_turso::query!("INSERT INTO board_intent_scan_cursors(project_id,epoch,position) VALUES (?,?,?) ON CONFLICT DO NOTHING", &row.project_id, &row.epoch, &row.position).execute(scope.executor()).await.map_err(StoreError::backend)?;
    }
    Ok(())
}

pub(crate) async fn read(
    scope: &mut TursoScope<'_, MessageCopyHost>,
) -> Result<MessageRows, StoreError> {
    let invalid = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "invalid!: i64" FROM router_pushes WHERE retention != 'creationAge' OR cancelled_after_delivery_json IS NOT NULL"#).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
    if invalid != 0 {
        return Err(StoreError::backend(crate::message_rows::invalid(
            "copied push additions",
        )));
    }
    let ids = sqlx_turso::query_scalar!(r#"SELECT progress_id AS "progress_id!: String" FROM subscription_delivery_progress UNION ALL SELECT progress_id AS "progress_id!: String" FROM board_intent_progress"#).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
    for id in ids {
        let valid = uuid::Uuid::parse_str(&id).is_ok_and(|value| {
            value.get_version_num() == 7
                && value.get_variant() == uuid::Variant::RFC4122
                && value.to_string() == id
        });
        if !valid {
            return Err(StoreError::backend(crate::message_rows::invalid(
                "worker UUIDv7",
            )));
        }
    }
    let pushes = sqlx_turso::query_as!(LegacyPushRow, r#"SELECT push_id AS "push_id!: String",kind AS "kind!: String",origin_kind AS "origin_kind!: String",origin_service_id AS "origin_service_id?: String",origin_endpoint_id AS "origin_endpoint_id?: String",origin_session_id AS "origin_session_id?: String",origin_router_ref AS "origin_router_ref?: String",target_service_id AS "target_service_id!: String",target_endpoint_id AS "target_endpoint_id!: String",target_session_id AS "target_session_id!: String",dm_delivery_mode AS "dm_delivery_mode?: String",dm_generation_guard_json AS "dm_generation_guard_json?: String",reply_to_push_id AS "reply_to_push_id?: String",header_facts_json AS "header_facts_json!: String",body AS "body?: String",ranges_json AS "ranges_json?: String",delivery_state AS "delivery_state!: String",last_outcome_json AS "last_outcome_json?: String",created_at AS "created_at!: String",settled_at AS "settled_at?: String",read_at AS "read_at?: String" FROM router_pushes ORDER BY push_id"#).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
    for row in &pushes {
        row.validate().map_err(StoreError::backend)?;
    }
    let subscriptions = sqlx_turso::query_as!(SubscriptionRow, r#"SELECT project_id AS "project_id!: String",membership_id AS "membership_id!: String",activated_epoch AS "activated_epoch!: i64",activated_position AS "activated_position!: i64",payload AS "payload!: String" FROM subscription_delivery_progress ORDER BY project_id,membership_id,activated_epoch,activated_position"#).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
    for row in &subscriptions {
        row.validate().map_err(StoreError::backend)?;
    }
    let intents = sqlx_turso::query_as!(IntentRow, r#"SELECT project_id AS "project_id!: String",intent_id AS "intent_id!: String",state_epoch AS "state_epoch!: i64",state_position AS "state_position!: i64" FROM board_intent_progress ORDER BY project_id,intent_id,state_epoch,state_position"#).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
    for row in &intents {
        row.validate().map_err(StoreError::backend)?;
    }
    let cursors = sqlx_turso::query_as!(IntentCursorRow, r#"SELECT project_id AS "project_id!: String",epoch AS "epoch!: i64",position AS "position!: i64" FROM board_intent_scan_cursors ORDER BY project_id"#).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
    for row in &cursors {
        row.validate().map_err(StoreError::backend)?;
    }
    Ok(MessageRows {
        pushes,
        subscriptions,
        intents,
        cursors,
    })
}
