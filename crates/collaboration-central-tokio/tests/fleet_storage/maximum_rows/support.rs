//! Preserve supplied values through canonical decoding before measuring SQL rows.
use super::super::{
    metadata,
    support::{Fixture, Io, TestResult, bundle},
};
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::SqlValue;
use message_board::*;
use runtime_primitives::SyncTime;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx_core::{column::Column, query::query, row::Row};
use std::sync::Arc;

const RECORD_BUDGET: usize = 1024 * 1024;
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400a9";
fn maximum_router_time() -> TestResult<RecordTime> {
    // Central's Clock has millisecond precision. This is Chrono's maximum UTC
    // date, at the final millisecond; nanosecond precision is not claimed.
    Ok(serde_json::from_value(
        json!({"kind":"router","value":"+262142-12-31T23:59:59.999Z"}),
    )?)
}
pub(super) async fn open(path: &std::path::Path) -> TestResult<Fixture> {
    let time = maximum_router_time()?;
    let millis = time
        .as_router_time()
        .ok_or("maximum fixture needs Router time")?
        .timestamp_millis();
    Fixture::open_at(path, SyncTime::from_unix_millis(millis)).await
}
pub(super) fn name() -> TestResult<ResourceName> {
    Ok(ResourceName::try_from("\0".repeat(MAX_NAME_BYTES))?)
}
pub(super) fn description() -> TestResult<Description> {
    Ok(Description::try_from("\0".repeat(MAX_DESCRIPTION_BYTES))?)
}
pub(super) fn origin() -> TestResult<ActorOrigin> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from(SERVICE.to_owned())?,
        machine: MachineLabel::try_from("\"".repeat(MAX_NAME_BYTES))?,
    })
}
pub(super) fn actor() -> TestResult<RecordedActor> {
    Ok(RecordedActor::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(SERVICE.to_owned())?,
                endpoint_id: EndpointId::try_from("a".repeat(64))?,
            },
            session_id: SessionId::try_from("\u{0001}".repeat(4096))?,
        },
    })
}
pub(super) fn repository(index: usize) -> TestResult<NormalizedOrigin> {
    Ok(NormalizedOrigin::try_from(format!(
        "a.b/{index:02}/{}",
        "\u{0001}".repeat(4089)
    ))?)
}
pub(super) async fn admit(
    fixture: &Fixture,
    operation: FleetOperation,
) -> TestResult<(FleetRequest, DecisionEffect)> {
    let mut request = metadata::request(operation)?;
    request.actor = actor()?;
    let answer = fixture
        .fleet_request_from(
            &request,
            durable_steps::Uuid::now_v7(),
            "maximum_row",
            origin()?,
        )
        .await?;
    let FleetRequestResult::Decided(RequestDecision::Accepted {
        effects,
        change: ChangeOutcome::Changed,
    }) = answer
    else {
        return Err(format!("maximum-row operation did not commit: {answer:?}").into());
    };
    let [effect] = effects.entries() else {
        return Err("maximum-row operation did not return exactly one real effect".into());
    };
    Ok((request, effect.clone()))
}
pub(super) async fn workspace(fixture: &Fixture) -> TestResult<WorkspaceId> {
    let (request, _) = admit(
        fixture,
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Size fixture".to_owned())?,
            description: Description::try_from("Prerequisite".to_owned())?,
        },
    )
    .await?;
    Ok(request.workspace)
}
pub(super) async fn import_prerequisites(
    fixture: &Fixture,
) -> TestResult<(WorkspaceId, BoardId, ProjectId)> {
    let mut imported = bundle()?;
    let Some(fleet_admission::FleetImportRecord::Workspace { repositories, .. }) =
        imported.records.first_mut()
    else {
        return Err("missing import prerequisite workspace".into());
    };
    repositories.clear();
    assert!(matches!(
        fixture.import(&imported).await?,
        fleet_admission::FleetImportResult::Imported { .. }
    ));
    Ok((
        WorkspaceId::try_from("019f0000-0000-7000-8000-000000000a81".to_owned())?,
        BoardId::try_from("019f0000-0000-7000-8000-000000000a82".to_owned())?,
        ProjectId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?,
    ))
}
pub(super) async fn canonical_fixture(fixture: &Fixture, record: Record) -> TestResult<Record> {
    // Shape measurement follows A's native store-write method. Archive
    // admission and workflow behavior are observed separately.
    let mut expected = record.clone();
    let driver = collaboration_central_core::StepConformance::new(
        Arc::new(collaboration_central_tokio::TokioStorageHost::new(
            fixture.object.clone(),
        )),
        Arc::new(Io::default()),
        runtime_primitives::Clock::now(&Io::default()),
    );
    let written = driver.write_fleet_entity_fixture(record).await?;
    expected.envelope_mut().position = written.envelope().position.clone();
    assert_eq!(written, expected, "fixture writer changed a supplied field");
    Ok(expected)
}
pub(super) async fn maximum_directory_summary(
    fixture: &Fixture,
    project: &ProjectId,
) -> TestResult {
    let time = maximum_router_time()?;
    let summary = collaboration_central_core::CopyDirectorySummary {
        database: fleet_sync_protocol::DatabaseId::try_from(project.as_str().to_owned())?,
        summary: board_sync_protocol::BoardAnnotation::ProjectSummary {
            status: Some(StatusName::try_from("\"".repeat(MAX_NAME_BYTES))?),
            latest_record_at: time,
        },
        position: fleet_sync_protocol::ReplicationPosition::new(
            fleet_sync_protocol::Epoch::new(u64::MAX)?,
            fleet_sync_protocol::CommitSequence::new(u64::MAX),
        ),
        acked_at: SyncTime::from_unix_millis(
            time.as_router_time()
                .ok_or("summary needs Router time")?
                .timestamp_millis(),
        ),
    };
    // A constructed largest typed summary, not a claim of an enormous history.
    // The repeated real call decodes the stored value through its own owner.
    for _ in 0..2 {
        fixture.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"copy","value":project},"sourceFace":"copy","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"maximum_directory_summary"},
            "kind":"central.directory-summary","target":null,"request":summary
        }))?).await.map_err(|error|format!("maximum directory summary did not answer: {error:?}"))?;
    }
    let id = project.as_str().to_owned();
    let stored = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT copy_summary FROM project_directory WHERE project_id=?",
                )
                .bind(id)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<String, _>("copy_summary")
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(
        serde_json::from_str::<collaboration_central_core::CopyDirectorySummary>(&stored)?,
        summary
    );
    Ok(())
}
pub(super) fn envelope(
    request: &FleetRequest,
    effect: DecisionEffect,
    board: Option<BoardId>,
) -> TestResult<RecordEnvelope> {
    Ok(RecordEnvelope {
        id: effect.0.id,
        scope: RecordScope {
            workspace: request.workspace.clone(),
            board,
            project: None,
        },
        actor: request.actor.clone(),
        request: Some(request.request_id.clone()),
        position: effect.1,
        time: maximum_router_time()?,
    })
}
async fn read(fixture: &Fixture, request: Value) -> TestResult<BoardReadPage> {
    let bytes = fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"maximum_row_read"},
        "kind":"central.fleet-read","target":null,"request":request
    }))?).await.map_err(|error|format!("canonical maximum-row read failed: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeRowSize {
    kind: &'static str,
    current_row_bytes: usize,
    feed_row_bytes: usize,
    current_numeric_upper_bytes: usize,
    feed_numeric_upper_bytes: usize,
}
pub(super) async fn measure(fixture: &Fixture, kind: &'static str, expected: Record) -> TestResult {
    // Expected comes from supplied inputs and the normal admission receipt,
    // never from the stored payload that is being checked.
    let reference = expected.reference();
    let BoardReadPage::FleetSnapshot {
        records,
        next: None,
        ..
    } = read(fixture, json!({"kind":"snapshot","value":{"cursor":null}})).await?
    else {
        return Err("small maximum-row fixture did not produce a complete snapshot".into());
    };
    let actual = records
        .into_iter()
        .map(|payload| payload.decode::<Record>())
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|record| record.reference() == reference);
    assert_eq!(
        actual,
        Some(expected.clone()),
        "canonical current decoder changed supplied values"
    );
    let position = expected
        .envelope()
        .position
        .fleet_position()
        .ok_or("maximum row has no Fleet placement")?
        .get();
    let BoardReadPage::FleetFeed(FleetFeedRead::Page {records,..}) =
        read(fixture,json!({"kind":"feed","value":{"after":position.checked_sub(1).ok_or("zero stored position")?}})).await? else {
            return Err("maximum row's real history could not be decoded".into());
        };
    assert_eq!(
        records
            .into_iter()
            .next()
            .ok_or("missing maximum feed record")?
            .decode::<Record>()?,
        expected
    );
    let sql = match reference.kind {
        RecordKind::Workspace => "SELECT * FROM workspaces WHERE workspace_id=?",
        RecordKind::Board => "SELECT * FROM boards WHERE board_id=?",
        RecordKind::Adr => "SELECT * FROM workspace_adrs WHERE adr_id=?",
        RecordKind::ProcessDefinition => "SELECT * FROM process_definitions WHERE definition_id=?",
        RecordKind::Project => "SELECT * FROM project_directory WHERE project_id=?",
        _ => return Err("maximum-row measurement names a non-Fleet family".into()),
    };
    let (rows, stored_digest) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let current = query::<sqlx_turso::Turso>(sql)
                    .bind(reference.id.as_str())
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                let digest = current
                    .try_get::<Option<String>, _>("import_digest")
                    .map_err(StoreError::backend)?;
                let feed = query::<sqlx_turso::Turso>(
                    "SELECT * FROM fleet_feed WHERE record_id=? AND position=?",
                )
                .bind(reference.id.as_str())
                .bind(i64::try_from(position).map_err(StoreError::backend)?)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                let rows = [current, feed]
                    .into_iter()
                    .map(|row| {
                        row.columns()
                            .iter()
                            .map(|column| {
                                if column.name() == "position" {
                                    row.try_get::<i64, _>(column.name())
                                        .map(SqlValue::Integer)
                                        .map_err(StoreError::backend)
                                } else {
                                    row.try_get::<Option<String>, _>(column.name())
                                        .map(|value| value.map_or(SqlValue::Null, SqlValue::Text))
                                        .map_err(StoreError::backend)
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((rows, digest))
            })
        })
        .await?;
    let [current, feed] = rows.as_slice() else {
        return Err("missing physical rows".into());
    };
    let imported_family = matches!(
        &expected,
        Record::Workspace(_) | Record::Board(_) | Record::DirectoryEntry(_)
    );
    assert_eq!(
        stored_digest
            .as_ref()
            .is_some_and(|digest| digest.len() == 64),
        imported_family,
        "maximum importable rows must retain their full original digest column"
    );
    let size = NativeRowSize {
        kind,
        current_row_bytes: encoded_size(current)?,
        feed_row_bytes: encoded_size(feed)?,
        current_numeric_upper_bytes: numeric_upper(current)?,
        feed_numeric_upper_bytes: numeric_upper(feed)?,
    };
    eprintln!(
        "CENTRAL_MAXIMUM_RECORD_SIZE {}",
        serde_json::to_string(&size)?
    );
    assert!(
        size.current_numeric_upper_bytes <= RECORD_BUDGET,
        "{kind} current row exceeds budget"
    );
    assert!(
        size.feed_numeric_upper_bytes <= RECORD_BUDGET,
        "{kind} feed row exceeds budget"
    );
    Ok(())
}
fn encoded_size(row: &[SqlValue]) -> TestResult<usize> {
    let bytes = serde_json::to_vec(row)?;
    assert_eq!(serde_json::from_slice::<Vec<SqlValue>>(&bytes)?, row);
    Ok(bytes.len())
}
fn maximum_integer_width(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            *number = serde_json::Number::from(i64::MIN)
        }
        Value::Array(values) => values.iter_mut().for_each(maximum_integer_width),
        Value::Object(values) => values.values_mut().for_each(maximum_integer_width),
        _ => {}
    }
}
fn numeric_upper(row: &[SqlValue]) -> TestResult<usize> {
    // A separate conservative reservation, matching A's method. These wider
    // integers are not presented as values that passed domain admission.
    let reserved = row
        .iter()
        .map(|cell| -> TestResult<SqlValue> {
            match cell {
                SqlValue::Integer(_) => Ok(SqlValue::Integer(i64::MIN)),
                SqlValue::Text(text) => {
                    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
                        return Ok(cell.clone());
                    };
                    if value.is_object() || value.is_array() {
                        maximum_integer_width(&mut value);
                        Ok(SqlValue::Text(serde_json::to_string(&value)?))
                    } else {
                        Ok(cell.clone())
                    }
                }
                _ => Ok(cell.clone()),
            }
        })
        .collect::<TestResult<Vec<_>>>()?;
    let bytes = encoded_size(&reserved)?;
    assert!(bytes >= encoded_size(row)?);
    Ok(bytes)
}
