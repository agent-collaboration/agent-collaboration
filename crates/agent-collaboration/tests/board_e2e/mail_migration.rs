//! LM01: real Host migrates an owned historical automation file, then reopens it.
use super::{
    mail_fixture::{MailFixture, at, set_field},
    mail_messages::{cli_send, terminal_readback},
};
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use serde_json::{Value, json};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../automation-storage/migrations");
const PRE_WAKE_VERSION: i64 = 20_261_015_000_200;
const TERMINAL_ITEM: &str = "019f0000-0000-7000-8000-000000000101";
const TERMINAL_TEXT: &str = "LM01 terminal message stays terminal through legacy wake migration";
const OLDER: &str = "019f0000-0000-7000-8000-000000000111";
const NEWEST: &str = "019f0000-0000-7000-8000-000000000112";
const FAST: &str = "019f0000-0000-7000-8000-000000000113";
const FAR: &str = "019f0000-0000-7000-8000-000000000114";
const CANCELLED: &str = "019f0000-0000-7000-8000-000000000115";

async fn seed_wake(
    connection: &mut SqliteConnection,
    target: &Value,
    id: &str,
    name: &str,
    now: i64,
) -> FixtureResult {
    let older = id == OLDER;
    let created = now - if older { 2000 } else { 1000 };
    let timing = match id {
        NEWEST => json!({"kind":"interval","seconds":300}),
        FAST => json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
        FAR => json!({"kind":"after","seconds":259200}),
        _ => json!({"kind":"after","seconds":600}),
    };
    let expiry = match id {
        NEWEST => Some(now + 30 * 86_400_000),
        FAST => Some(now + 7_200_000),
        _ => None,
    };
    let status = match id {
        OLDER => "paused",
        CANCELLED => "cancelled",
        _ => "active",
    };
    let due = (id != CANCELLED).then_some(now + if id == FAR { 259_200_000 } else { 300_000 });
    let change = uuid::Uuid::now_v7().to_string();
    let mut recipient = target.clone();
    set_field(&mut recipient, "sessionId", json!(name))?;
    let definition = json!({"wakeupId":id,"changeId":change,"message":{"target":recipient,
        "content":{"kind":"humanUser","text":"LM01 literal legacy wake"},"delivery":"auto"},
        "timing":timing,"anchorAtMs":created,"expiresAtMs":expiry,"createdAtMs":created});
    sqlx::query("INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,?,?,?,?,?,?,NULL,NULL,?,?)")
        .bind(id).bind(change).bind(status).bind(definition.to_string()).bind(created).bind(created)
        .bind(due).bind(expiry).bind(created).bind(created).execute(connection).await?;
    Ok(())
}

async fn historical_store(suite: &MailFixture) -> FixtureResult {
    let directory = suite.stopped_directory()?;
    let path = directory.join("automation.sqlite");
    std::fs::rename(&path, directory.join("pristine-automation.sqlite"))?;
    let connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await?;
    run_scenario(
        connection,
        async |connection: &mut SqliteConnection| {
            MIGRATOR
                .run_direct(Some(PRE_WAKE_VERSION), &mut *connection, false)
                .await?;
            let now = chrono::Utc::now().timestamp_millis();
            for (id, name) in [
                (OLDER, "lm01-duplicate"),
                (NEWEST, "lm01-duplicate"),
                (FAST, "lm01-fast"),
                (FAR, "lm01-far"),
                (CANCELLED, "lm01-terminal"),
            ] {
                seed_wake(connection, &suite.target, id, name, now).await?;
            }
            Ok(())
        },
        async |connection| {
            connection.close().await?;
            Ok(())
        },
        || Ok(()),
    )
    .await
}

fn timestamp(value: &Value, pointer: &str) -> FixtureResult<i64> {
    Ok(chrono::DateTime::parse_from_rfc3339(
        at(value, pointer)?
            .as_str()
            .ok_or("legacy wake timestamp absent")?,
    )?
    .timestamp_millis())
}

async fn observe_wakes(suite: &MailFixture, before: i64, after: i64) -> FixtureResult<Vec<Value>> {
    let mut snapshots = Vec::new();
    for (id, state) in [
        (OLDER, "expired"),
        (NEWEST, "active"),
        (FAST, "expired"),
        (FAR, "active"),
        (CANCELLED, "cancelled"),
    ] {
        let shown = suite
            .host()?
            .mcp("wake_show", json!({"wakeupId":id}))
            .await?;
        assert_eq!(shown.pointer("/definition/wakeupId"), Some(&json!(id)));
        assert_eq!(at(&shown, "/state")?, &json!(state));
        assert_eq!(
            shown.pointer("/definition/createdBy"),
            Some(&json!({"kind":"unrecorded"}))
        );
        if matches!(id, NEWEST | FAR) {
            let cap = if id == NEWEST {
                604_800_000
            } else {
                86_400_000
            };
            assert!(
                (before + cap..=after + cap).contains(&timestamp(&shown, "/definition/expiresAt")?)
            );
        }
        if matches!(id, OLDER | FAST | FAR | CANCELLED) {
            assert_eq!(at(&shown, "/nextDueAt")?, &Value::Null);
        }
        snapshots.push(shown);
    }
    Ok(snapshots)
}

async fn stopped_migration_readback(suite: &MailFixture) -> FixtureResult {
    let connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(suite.stopped_directory()?.join("automation.sqlite"))
            .read_only(true),
    )
    .await?;
    run_scenario(connection, async |connection: &mut SqliteConnection| {
        for (id, event) in [(OLDER,"upgradeDuplicate"),(FAST,"upgradeLimit")] {
            let rows = sqlx::query("SELECT event_kind,event_body_json FROM automation_events WHERE subject_id=? AND event_kind=?")
                .bind(id).bind(event).fetch_all(&mut *connection).await?;
            assert_eq!(rows.len(), 1, "exactly one {event} migration event");
            let row = rows.first().ok_or("legacy wake event absent")?;
            assert_eq!(row.try_get::<String, _>("event_kind")?, event);
            let body: Value = serde_json::from_str(&row.try_get::<String, _>("event_body_json")?)?;
            assert_eq!(at(&body, "/after")?, &json!("expired"));
        }
        let versions: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version").fetch_all(&mut *connection).await?;
        assert_eq!(versions.len(), 13);
        assert_eq!(versions.last(), Some(&20261023000100));
        let release_ms: i64 = sqlx::query_scalar("SELECT recorded_at_ms FROM automation_events WHERE subject_id=? AND event_kind='upgradeDuplicate'")
            .bind(OLDER).fetch_one(&mut *connection).await?;
        for (id, lifetime_ms) in [(NEWEST, 604_800_000_i64), (FAR, 86_400_000)] {
            let expiry_ms: i64 = sqlx::query_scalar("SELECT expires_at_ms FROM wakeup_definitions WHERE wakeup_id=?")
                .bind(id).fetch_one(&mut *connection).await?;
            assert_eq!(expiry_ms, release_ms + lifetime_ms, "migration cap uses its actual committed release instant");
        }
        let legacy_message_tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('router_pushes','mail_items','subscription_delivery_progress','board_intent_progress','board_intent_scan_cursors')").fetch_one(&mut *connection).await?;
        assert_eq!(legacy_message_tables, 0);
        Ok(())
    }, async |connection| { connection.close().await?; Ok(()) }, || Ok(())).await
}

pub async fn legacy_migration(suite: &mut MailFixture) -> FixtureResult {
    let original = cli_send(suite, TERMINAL_ITEM, TERMINAL_TEXT).await?;
    suite.peer_mut()?.expect_marker(TERMINAL_TEXT).await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    terminal_readback(suite, TERMINAL_ITEM, &original).await?;
    historical_store(suite).await?;
    let before = chrono::Utc::now().timestamp_millis();
    suite.reopen_host().await?;
    let after = chrono::Utc::now().timestamp_millis();
    let snapshots = observe_wakes(suite, before, after).await?;
    assert_eq!(
        cli_send(suite, TERMINAL_ITEM, TERMINAL_TEXT).await?,
        original
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    stopped_migration_readback(suite).await?;
    terminal_readback(suite, TERMINAL_ITEM, &original).await?;
    suite.reopen_host().await?;
    let reopened = observe_wakes(suite, before, after).await?;
    for (first, second) in snapshots.iter().zip(&reopened) {
        assert_eq!(
            at(first, "/definition")?,
            at(second, "/definition")?,
            "reopen must not reanchor migration caps"
        );
        assert_eq!(at(first, "/state")?, at(second, "/state")?);
    }
    assert_eq!(
        cli_send(suite, TERMINAL_ITEM, TERMINAL_TEXT).await?,
        original
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    stopped_migration_readback(suite).await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    Ok(())
}
