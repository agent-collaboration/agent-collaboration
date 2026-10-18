//! M2.5b preserves project_records' metadata, corruption and bounded hierarchy claims.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{FleetStorageError, FleetStore};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

fn fleet_request(
    workspace: WorkspaceId,
    operation: FleetOperation,
) -> Result<FleetRequest, TestError> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace,
        actor: RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("metadata proof".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
async fn create_workspace(store: &FleetStore, workspace: &WorkspaceId) -> Result<(), TestError> {
    let reply = store
        .admit(
            &fleet_request(
                workspace.clone(),
                FleetOperation::WorkspaceCreate {
                    name: ResourceName::try_from("Router".to_owned())?,
                    description: Description::try_from("Communication".to_owned())?,
                },
            )?,
            chrono::Utc::now(),
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Corrupt identity must be attributed without echoing stored raw values.
async fn malformed_stored_primary_id_is_reported_without_echoing_the_raw_value()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture._directory.path().join("fleet-cache.sqlite"))
        .connect()
        .await?;
    sqlx::query("INSERT INTO cached_fleet_records(kind,id,payload) SELECT kind,'malformed',payload FROM cached_fleet_records WHERE kind='workspace' AND id=?").bind(fixture.workspace.as_str()).execute(&mut raw).await?;
    raw.close().await?;
    let failure = fixture
        .api
        .call(
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{failure}"
    );
    let message = failure
        .pointer("/error/data/message")
        .and_then(Value::as_str)
        .ok_or("message")?;
    assert!(!message.contains("malformed"));
    assert_eq!(
        failure.pointer("/error/data/details/field"),
        Some(&json!("workspaceId")),
        "{failure}"
    );
    fixture.stop().await
}
async fn overwrite_name(
    path: &std::path::Path,
    workspace: &WorkspaceId,
    name: &str,
) -> Result<(), TestError> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .connect()
        .await?;
    let payload: String =
        sqlx::query_scalar("SELECT payload FROM fleet_records WHERE kind='workspace' AND id=?")
            .bind(workspace.as_str())
            .fetch_one(&mut connection)
            .await?;
    let mut stored: Value = serde_json::from_str(&payload)?;
    *stored
        .pointer_mut("/record/name")
        .ok_or("workspace name missing")? = json!(name);
    sqlx::query("UPDATE fleet_records SET payload=? WHERE kind='workspace' AND id=?")
        .bind(stored.to_string())
        .bind(workspace.as_str())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn corrupt_metadata_listing_names_the_resource_and_canonical_inspection_action()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let path = fixture._directory.path().join("fleet-cache.sqlite");
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM cached_fleet_records WHERE kind='workspace' AND id=?",
    )
    .bind(fixture.workspace.as_str())
    .fetch_one(&mut connection)
    .await?;
    let mut stored: Value = serde_json::from_str(&payload)?;
    *stored
        .pointer_mut("/record/name")
        .ok_or("stored name missing")? = json!("");
    sqlx::query("UPDATE cached_fleet_records SET payload=? WHERE kind='workspace' AND id=?")
        .bind(stored.to_string())
        .bind(fixture.workspace.as_str())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let failure = fixture
        .api
        .call(
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{failure}"
    );
    assert_eq!(
        failure.pointer("/error/data/stage"),
        Some(&json!("inspection"))
    );
    assert_eq!(
        failure.pointer("/error/data/nextAction"),
        Some(&json!("inspectResource"))
    );
    assert_eq!(
        failure.pointer("/error/data/details"),
        Some(
            &json!({"kind":"resource","resource":{"kind":"workspace","workspaceId":fixture.workspace}})
        )
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn workspace_identity_and_metadata_survive_reopen() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let workspace = WorkspaceId::generate();
    let store = FleetStore::open(&path).await?;
    create_workspace(&store, &workspace).await?;
    store.close().await;
    let reopened = FleetStore::open(&path).await?;
    let record = reopened
        .record(&workspace.clone().into())
        .await?
        .ok_or("workspace missing")?;
    assert!(
        matches!(record, Record::Workspace(row) if row.workspace_id==workspace && row.name.as_str()=="Router" && row.description.as_str()=="Communication")
    );
    let renamed = reopened
        .admit(
            &fleet_request(
                workspace.clone(),
                FleetOperation::WorkspaceUpdate {
                    name: ResourceName::try_from("Renamed".to_owned())?,
                    description: Description::try_from("Updated".to_owned())?,
                },
            )?,
            chrono::Utc::now(),
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    assert!(matches!(
        renamed.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let expected = renamed.records.first().ok_or("renamed record missing")?;
    assert_eq!(
        reopened.record(&workspace.into()).await?.as_ref(),
        Some(expected)
    );
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn corrupted_stored_name_is_rejected_by_domain_decoding() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let workspace = WorkspaceId::generate();
    let store = FleetStore::open(&path).await?;
    create_workspace(&store, &workspace).await?;
    store.close().await;
    overwrite_name(&path, &workspace, "").await?;
    let reopened = FleetStore::open(&path).await?;
    assert!(matches!(
        reopened.snapshot_records().await,
        Err(FleetStorageError::InvalidRecordAt { .. })
    ));
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn noncanonical_stored_name_whitespace_is_rejected_instead_of_trimmed()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let workspace = WorkspaceId::generate();
    let store = FleetStore::open(&path).await?;
    create_workspace(&store, &workspace).await?;
    store.close().await;
    overwrite_name(&path, &workspace, " padded ").await?;
    let reopened = FleetStore::open(&path).await?;
    assert!(matches!(
        reopened.record(&workspace.into()).await,
        Err(FleetStorageError::InvalidRecordAt { .. })
    ));
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn large_metadata_lists_continue_before_the_control_frame_limit() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let full_description = "x".repeat(MAX_DESCRIPTION_BYTES);
    let mut update = fixture.header();
    update.as_object_mut().ok_or("update object")?.extend(json!({"workspaceId":fixture.workspace,"name":"test workspace","description":full_description}).as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call("workspace_update", update).await?);
    let mut update = fixture.header();
    update.as_object_mut().ok_or("update object")?.extend(
        json!({"boardId":fixture.board,"name":"test board","description":full_description})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    accepted(&fixture.api.call("board_update", update).await?);
    for index in 0..60 {
        for (tool, fields) in [
            (
                "workspace_create",
                json!({"workspaceId":WorkspaceId::generate(),"name":format!("Large workspace {index:02}"),"description":full_description}),
            ),
            (
                "board_create",
                json!({"workspaceId":fixture.workspace,"boardId":BoardId::generate(),"name":format!("Large board {index:02}"),"description":full_description,"kind":{"kind":"area"}}),
            ),
            (
                "project_create",
                json!({"boardId":fixture.board,"projectId":ProjectId::generate(),"title":format!("Large project {index:02}")}),
            ),
        ] {
            let mut request = fixture.header();
            request
                .as_object_mut()
                .ok_or("request object")?
                .extend(fields.as_object().ok_or("fields")?.clone());
            accepted(&fixture.api.call(tool, request).await?);
        }
    }
    for (tool, mut request, expected_count) in [
        (
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
            61,
        ),
        (
            "board_list",
            json!({"workspaceId":fixture.workspace,"includeArchived":true,"page":{"limit":100,"cursor":null}}),
            61,
        ),
        // Project directory rows no longer hold descriptions: ordinary bounded paging still traverses all rows.
        (
            "project_list",
            json!({"boardId":fixture.board,"page":{"limit":1,"cursor":null}}),
            60,
        ),
    ] {
        let mut count = 0;
        let mut pages = 0;
        loop {
            let response = fixture.api.call(tool, request.clone()).await?;
            assert!(
                serde_json::to_vec(&response)?.len()
                    < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
            );
            let records = response
                .pointer("/result/page/records")
                .and_then(Value::as_array)
                .ok_or("metadata records missing")?;
            count += records.len();
            pages += 1;
            let cursor = response
                .pointer("/result/page/nextCursor")
                .ok_or("cursor missing")?;
            if cursor.is_null() {
                break;
            }
            *request
                .pointer_mut("/page/cursor")
                .ok_or("request cursor missing")? = cursor.clone();
        }
        assert_eq!(count, expected_count, "{tool}");
        assert!(pages > 1, "{tool} did not continue");
    }
    fixture.stop().await
}
