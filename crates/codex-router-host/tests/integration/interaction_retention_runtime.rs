//! The Host trims settled interaction records at their retention cutoff on its one injected
//! clock, and leaves current records alone.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_protocol::{
    EndpointId, EndpointRef, MessageDelivery, OperationId, PushHeaderFacts, PushId, PushKind,
    PushOrigin, PushRecordDraft, SessionId, SessionRef, UuidIdentity,
};
use sqlx::Connection;
use std::{os::unix::fs::DirBuilderExt, path::Path, time::Duration};

type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error>>;

/// Bounds a hang; the Host's clock, not this wait, decides when the push is due.
const BOUND: Duration = Duration::from_secs(60);

fn session(endpoint: &str, id: &str) -> TestResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())?,
            endpoint_id: EndpointId::try_from(endpoint.to_owned())?,
        },
        session_id: SessionId::try_from(id.to_owned())?,
    })
}

fn direct_message(created_at: &str) -> TestResult<PushRecordDraft> {
    Ok(PushRecordDraft {
        mode: Some(MessageDelivery::Auto),
        guard: None,
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())?,
        kind: PushKind::DirectMessage,
        origin: PushOrigin::Session(session("claude-local", "sender")?),
        origin_router_ref: None,
        target: session("codex-local", "target")?,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("retained message".to_owned()),
        activity: None,
        created_at: created_at.parse()?,
    })
}

/// Stores `draft` as a delivered push, as an earlier Host run left it.
async fn seed_delivered(directory: &Path, draft: PushRecordDraft) -> TestResult<PushId> {
    let push_id = draft.push_id.clone();
    let store =
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite")).await?;
    store.insert_push_record(draft).await?;
    store.close().await?;
    let mut writer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(directory.join("interaction.sqlite")),
    )
    .await?;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET delivery_state='delivered' WHERE push_id=?",
    )
    .bind(push_id.as_str())
    .execute(&mut writer)
    .await?;
    writer.close().await?;
    Ok(push_id)
}

async fn stored(directory: &Path, push_id: &PushId) -> TestResult<bool> {
    let mut reader = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(directory.join("interaction.sqlite"))
            .read_only(true),
    )
    .await?;
    let count: i64 = sqlx::query_scalar::<sqlx_turso::Turso, i64>(
        "SELECT COUNT(*) FROM router_pushes WHERE push_id=?",
    )
    .bind(push_id.as_str())
    .fetch_one(&mut reader)
    .await?;
    reader.close().await?;
    Ok(count == 1)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_trims_a_settled_push_at_its_cutoff_on_its_clock_and_keeps_current_ones() -> TestResult
{
    // Arrange: the Host's clock starts at 2026-11-04T00:00:00Z (host_clock_control). One push
    // reaches its two-calendar-month cutoff one second later; the other is current. An
    // unreadable automation store sits beside them: retention must not depend on it, as it did
    // when the automation worker owned this trimming.
    let root = std::path::PathBuf::from("/tmp").join(format!(
        "interaction-retention-{}",
        OperationId::generate().as_str()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    std::fs::write(root.join("automation.sqlite"), b"not a database")?;
    let aging = seed_delivered(&root, direct_message("2026-09-04T00:00:01Z")?).await?;
    let current = seed_delivered(&root, direct_message("2026-11-03T00:00:00Z")?).await?;
    let clock_listener = crate::host_clock_control::prepare(&root)?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("unavailable.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let mut clock = crate::host_clock_control::HostClockControl::connected(clock_listener).await?;
    let before_cutoff = stored(&root, &aging).await?;

    // Act: the Host's clock passes the cutoff.
    clock.advance(Duration::from_secs(2)).await?;
    let trimmed = tokio::time::timeout(BOUND, async {
        while stored(&root, &aging).await? {
            tokio::task::yield_now().await;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;

    // Assert
    let current_stays = stored(&root, &current).await?;
    runtime.shutdown().await?;
    std::fs::remove_dir_all(&root)?;
    if !before_cutoff {
        return Err("a push went before its cutoff".into());
    }
    if !matches!(trimmed, Ok(Ok(()))) {
        return Err(format!("the Host never trimmed the settled push: {trimmed:?}").into());
    }
    if !current_stays {
        return Err("a current push was trimmed".into());
    }
    Ok(())
}
