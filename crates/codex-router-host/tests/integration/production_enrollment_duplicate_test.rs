//! An isolated production Host exposes the same terminal replacement on both status paths.
use super::*;
use board_fleet_sync::{BoardProjectSchema, ReqwestSessionTransport};
use board_sync_protocol::{
    BoardConsumerProtocol, FleetLinkRecord, FleetLinkRefusal, FleetLinkState,
};
use fleet_protocol::{ClientFrame, CloseReason, SchemaSupport, ServerFrame, SessionId};
use fleet_session::{SessionTransport, TransportConnection};
use sqlx::{Connection, Row};

#[tokio::test]
async fn newer_live_session_is_retained_as_refused_by_host_and_installation_status() -> TestResult {
    let fixture = ProductionEnrollmentFixture::start(EnrollmentLocation::RouterRoot).await?;
    fixture
        .run(async |fixture| {
            fixture.host.await_ready().await?;
            let client =
                CollaborationClient::connect(&fixture.directory, "duplicate-link-test", "1")
                    .await?;
            await_state(&client, |state| {
                matches!(state, FleetLinkState::Connected { .. })
            })
            .await?;
            let inputs = fixture.central.session_inputs().map_err(io::Error::other)?;
            let transport = ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?;
            let mut replacement = transport
                .connect(&inputs.endpoint, &inputs.credential)
                .await?;
            let session_id = SessionId::new(uuid::Uuid::now_v7())?;
            let max_version = BoardProjectSchema::new()?
                .supported_schemas()
                .iter()
                .map(|schema| schema.version)
                .max()
                .ok_or("empty board schema")?;
            replacement
                .send(
                    serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                        session_id,
                        schema_support: SchemaSupport { max_version },
                    })?
                    .into(),
                )
                .await?;
            let welcome = tokio::time::timeout(BOUND, replacement.receive())
                .await?
                .ok_or("replacement Welcome missing")??;
            require(
                matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&welcome)?,
            ServerFrame::Welcome { session_id: actual, .. } if actual == session_id),
                "central did not welcome replacement session",
            )?;
            let refused = await_state(&client, |state| {
                matches!(
                    state,
                    FleetLinkState::Refused {
                        reason: FleetLinkRefusal::IdentityInUseElsewhere,
                        ..
                    }
                )
            })
            .await?;
            let frames = operator_client::send_operator_request(
                &fixture.host.operator,
                OperatorRequest::Status,
                BOUND,
            )
            .await?;
            let host_link = frames
                .iter()
                .find_map(|frame| match frame {
                    OperatorFrame::Terminal(response) => match response.snapshot().fleet() {
                        codex_router_host::FleetHostObservation::Available {
                            central: Some(record),
                            ..
                        } => Some(record.as_ref()),
                        _ => None,
                    },
                    OperatorFrame::Progress(_) => None,
                })
                .ok_or("Host link observation unavailable")?;
            require(
                host_link == &refused,
                "Host and installation status differ after replacement",
            )?;
            require_persisted_duplicate_refusal(&fixture.directory, &refused).await?;
            require(
                refused.peer == fixture.peer,
                "replacement changed the installation peer",
            )?;
            replacement.close(CloseReason::ClientClosed).await;
            Ok(())
        })
        .await
}

async fn await_state(
    client: &CollaborationClient,
    expected: impl Fn(&FleetLinkState) -> bool,
) -> TestResult<FleetLinkRecord> {
    tokio::time::timeout(BOUND, async {
        loop {
            let record = client
                .installation_show()
                .await?
                .central
                .into_record()
                .ok_or("link observation missing")?;
            if expected(&record.state) {
                return Ok(record);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn require_persisted_duplicate_refusal(
    directory: &Path,
    expected: &FleetLinkRecord,
) -> TestResult {
    // The live Host owns Turso's writer. Inspect the committed file through the
    // same independent read-only SQLite boundary this process fixture used before.
    let mut database = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("fleet-cache.sqlite"))
            .read_only(true),
    )
    .await?;
    let row = sqlx::query("SELECT peer_id, endpoint, instance, connection, revision, state_kind, state_since, connecting_attempt, last_frame_at, refusal_kind, refusal_detail, last_drop_kind, last_drop_at, last_drop_connection, last_failure_kind, last_failure_detail, last_failure_at FROM fleet_link_state")
        .fetch_one(&mut database).await?;
    database.close().await?;
    let FleetLinkState::Refused {
        at,
        reason: FleetLinkRefusal::IdentityInUseElsewhere,
    } = &expected.state
    else {
        return Err("expected duplicate-refusal record missing".into());
    };
    require(
        row.try_get::<String, _>("peer_id")? == expected.peer.to_string()
            && row.try_get::<String, _>("endpoint")? == expected.endpoint
            && row.try_get::<String, _>("instance")? == expected.instance.to_string()
            && row.try_get::<String, _>("connection")? == expected.connection.to_string()
            && row.try_get::<String, _>("revision")? == expected.revision.to_string(),
        "committed native link identity or counters differ from public observation",
    )?;
    require(
        row.try_get::<String, _>("state_kind")? == "refused"
            && stored_time(&row, "state_since")? == *at
            && row
                .try_get::<Option<i64>, _>("connecting_attempt")?
                .is_none()
            && stored_optional_time(&row, "last_frame_at")? == expected.last_frame_at
            && row.try_get::<Option<String>, _>("refusal_kind")?.as_deref()
                == Some("identityInUseElsewhere")
            && row
                .try_get::<Option<String>, _>("refusal_detail")?
                .is_none(),
        "duplicate refusal or last frame was not committed",
    )?;
    let dropped = expected
        .last_drop
        .as_ref()
        .ok_or("replacement drop missing")?;
    require(
        row.try_get::<Option<String>, _>("last_drop_kind")?
            .as_deref()
            == Some(dropped.reason.as_str())
            && stored_optional_time(&row, "last_drop_at")? == Some(dropped.at)
            && row
                .try_get::<Option<String>, _>("last_drop_connection")?
                .as_deref()
                == Some(dropped.connection.to_string().as_str()),
        "replacement drop metadata was not committed",
    )?;
    let failure = expected
        .last_failure
        .as_ref()
        .ok_or("replacement failure missing")?;
    let detail = match &failure.reason {
        board_sync_protocol::FleetLinkFailureReason::ReplacedByNewerSession => None,
        board_sync_protocol::FleetLinkFailureReason::SessionClosed {
            reason: CloseReason::ConnectionReplaced,
        } => Some("connectionReplaced"),
        other => return Err(format!("unexpected duplicate diagnostic: {other:?}").into()),
    };
    require(
        row.try_get::<Option<String>, _>("last_failure_kind")?
            .as_deref()
            == Some(failure.reason.as_str())
            && row
                .try_get::<Option<String>, _>("last_failure_detail")?
                .as_deref()
                == detail
            && stored_optional_time(&row, "last_failure_at")? == Some(failure.at),
        "typed replacement diagnostic was not committed",
    )
}

fn stored_time(
    row: &sqlx::sqlite::SqliteRow,
    name: &'static str,
) -> TestResult<chrono::DateTime<chrono::Utc>> {
    Ok(row.try_get::<String, _>(name)?.parse()?)
}
fn stored_optional_time(
    row: &sqlx::sqlite::SqliteRow,
    name: &'static str,
) -> TestResult<Option<chrono::DateTime<chrono::Utc>>> {
    Ok(row
        .try_get::<Option<String>, _>(name)?
        .map(|value| value.parse())
        .transpose()?)
}
