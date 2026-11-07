//! An isolated production Host exposes the same terminal replacement on both status paths.
use super::*;
use board_fleet_sync::{BoardProjectSchema, ReqwestSessionTransport};
use board_sync_protocol::{
    BoardConsumerProtocol, FleetLinkRecord, FleetLinkRefusal, FleetLinkState,
};
use fleet_sync_protocol::{ClientFrame, CloseReason, SchemaSupport, ServerFrame, SessionId};
use fleet_sync_session::{SessionTransport, TransportConnection};
use sqlx::Connection;

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
                        } => Some(record),
                        _ => None,
                    },
                    OperatorFrame::Progress(_) => None,
                })
                .ok_or("Host link observation unavailable")?;
            require(
                host_link == &refused,
                "Host and installation status differ after replacement",
            )?;
            require(
                persisted_link(&fixture.directory).await? == refused,
                "replacement refusal was not committed to FleetCache",
            )?;
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

async fn persisted_link(directory: &Path) -> TestResult<FleetLinkRecord> {
    let mut database = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("fleet-cache.sqlite"))
            .read_only(true),
    )
    .await?;
    let stored: String =
        sqlx::query_scalar("SELECT record FROM fleet_runtime_state WHERE kind = 'centralLink'")
            .fetch_one(&mut database)
            .await?;
    database.close().await?;
    Ok(serde_json::from_str(&stored)?)
}
