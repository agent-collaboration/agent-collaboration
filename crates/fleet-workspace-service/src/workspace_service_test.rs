use super::WorkspaceService;
use central_storage::{CorruptReason, StoreFailure};
use fleet_contracts::{ReadWorkspaceRequestStatus, RetainedWorkspaceStatus, WorkspaceServiceRpc};
use serde_json::json;
use sqlx::SqlitePool;
use workspace_model::{ActorIdentityId, ItemRef, RequestId, WorkspaceAnswer, WorkspaceResult};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const REQUEST: &str = "019f0000-0000-7000-8000-000000000011";
const OTHER_REQUEST: &str = "019f0000-0000-7000-8000-000000000012";
const ACTOR: &str = "019f0000-0000-7000-8000-000000000021";
const FOREIGN: &str = "019f0000-0000-7000-8000-000000000022";
const CUTOFF: &str = "2026-11-20T12:00:00+00:00";
const FRESH: &str = "2026-11-20T12:00:01+00:00";

async fn database() -> TestResult<SqlitePool> {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    fleet_storage_schema::workspace_migrations::workspace_migrator()
        .run(&db)
        .await?;
    Ok(db)
}

async fn status(
    db: &SqlitePool,
    request_id: &str,
    actor: &str,
) -> TestResult<Result<RetainedWorkspaceStatus, StoreFailure>> {
    let request = ReadWorkspaceRequestStatus {
        request_id: request_id.parse::<RequestId>()?,
        actor: actor.parse::<ActorIdentityId>()?,
        cutoff: CUTOFF.parse()?,
    };
    Ok(WorkspaceService::new(db).read_request_status(request).await)
}

async fn insert_receipt(
    db: &SqlitePool,
    request_id: &str,
    timestamp: &str,
    answer: &str,
) -> TestResult<()> {
    sqlx::query("INSERT INTO workspace_request_log (request_id,caller,content_digest,answer,created_at,updated_at) VALUES (?,?,'digest',?,?,?)")
        .bind(request_id).bind(ACTOR).bind(answer).bind(timestamp).bind(timestamp)
        .execute(db).await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact answer and cutoff assertions"
)]
async fn status_binds_the_request_and_strict_cutoff_to_their_own_positions() -> TestResult<()> {
    let db = database().await?;
    let answer = serde_json::to_string(&WorkspaceResult::Refused(
        workspace_model::RefusalReason::MachineRevoked,
    ))?;
    insert_receipt(&db, REQUEST, FRESH, &answer).await?;
    insert_receipt(&db, OTHER_REQUEST, CUTOFF, &answer).await?;

    assert_eq!(
        status(&db, REQUEST, ACTOR).await??,
        RetainedWorkspaceStatus::Refused(workspace_model::RefusalReason::MachineRevoked)
    );
    assert_eq!(
        status(&db, OTHER_REQUEST, ACTOR).await??,
        RetainedWorkspaceStatus::NotFound
    );
    assert_eq!(
        status(&db, REQUEST, FOREIGN).await??,
        RetainedWorkspaceStatus::NotFound
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact SQL and JSON decode failure assertions"
)]
async fn damaged_answer_is_corrupt_even_for_the_other_actor() -> TestResult<()> {
    let db = database().await?;
    insert_receipt(&db, REQUEST, FRESH, "invalid JSON").await?;
    for actor in [ACTOR, FOREIGN] {
        assert_eq!(
            status(&db, REQUEST, actor).await?,
            Err(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: "workspace_request_log.answer".to_owned()
                }
            })
        );
    }
    sqlx::query("UPDATE workspace_request_log SET answer = ? WHERE request_id = ?")
        .bind(vec![0xff_u8])
        .bind(REQUEST)
        .execute(&db)
        .await?;
    for actor in [ACTOR, FOREIGN] {
        assert_eq!(
            status(&db, REQUEST, actor).await?,
            Err(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: "answer".to_owned()
                }
            })
        );
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the exact wire shape read from migrated SQLite"
)]
async fn stored_success_and_missing_item_have_distinct_status_wire_tags() -> TestResult<()> {
    let db = database().await?;
    let workspace_id = "019f0000-0000-7000-8000-000000000031";
    let area_id = "019f0000-0000-7000-8000-000000000032";
    let item = ItemRef::Area(area_id.parse()?);
    let wire_item = json!({"kind": "area", "id": area_id});

    let receipts = [
        (
            REQUEST,
            WorkspaceResult::Done(WorkspaceAnswer::Unchanged { item: item.clone() }),
            json!({"kind": "done", "value": {"kind": "unchanged", "item": wire_item}}),
        ),
        (
            OTHER_REQUEST,
            WorkspaceResult::NotFound {
                workspace_id: workspace_id.parse()?,
                item,
            },
            json!({"kind": "itemNotFound", "value": {"workspaceId": workspace_id, "item": wire_item}}),
        ),
    ];
    for (request, result, expected) in receipts {
        insert_receipt(&db, request, FRESH, &serde_json::to_string(&result)?).await?;
        let retained = status(&db, request, ACTOR).await??;
        assert_eq!(serde_json::to_value(&retained)?, expected);
        assert_eq!(
            serde_json::from_value::<RetainedWorkspaceStatus>(expected)?,
            retained
        );
    }
    assert_eq!(
        status(&db, OTHER_REQUEST, FOREIGN).await??,
        RetainedWorkspaceStatus::NotFound
    );
    Ok(())
}

#[test]
fn status_wire_rejects_nonretained_results_and_unknown_fields() {
    let rejected = [
        json!({"kind": "answered", "value": {"kind": "conflict"}}),
        json!({"kind": "conflict"}),
        json!({"kind": "outcomeUnknown"}),
        json!({"kind": "centralUnavailable"}),
        json!({"kind": "notFound", "extra": true}),
        json!({"kind": "itemNotFound", "value": {
            "workspaceId": "019f0000-0000-7000-8000-000000000031",
            "item": {"kind": "area", "id": "019f0000-0000-7000-8000-000000000032"},
            "extra": true
        }}),
    ];
    for reply in rejected {
        let decoded = serde_json::from_value::<RetainedWorkspaceStatus>(reply.clone());
        assert!(
            decoded.is_err(),
            "unexpected retained status for {reply}: {decoded:?}"
        );
    }
}
