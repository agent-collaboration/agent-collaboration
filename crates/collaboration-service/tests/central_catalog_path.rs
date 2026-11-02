//! Catalog errors, write replay and response bounds exercise real SQLite and API carriers.
use collaboration_client::{CentralCatalogFailure, InstructionClientError};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, AutomationRefusal,
    InstructionCreateParams, InstructionSnapshot, OperationId,
};
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

struct LostInstructionReply {
    inner: Arc<dyn CentralAutomationTransport>,
    evidence: Arc<LostReplyEvidence>,
}
#[derive(Default)]
struct LostReplyEvidence {
    lost: AtomicBool,
    calls: AtomicUsize,
    committed: Mutex<Option<InstructionSnapshot>>,
}
impl CentralAutomationTransport for LostInstructionReply {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let creating = matches!(&request, AutomationCall::InstructionCreate(_));
            if creating {
                self.evidence.calls.fetch_add(1, Ordering::SeqCst);
            }
            let answer = self.inner.call(request).await?;
            if creating
                && let AutomationAnswer::Instruction(document) = &answer
                && !self.evidence.lost.swap(true, Ordering::SeqCst)
            {
                *self
                    .evidence
                    .committed
                    .lock()
                    .map_err(|_| CentralAutomationError::InvalidReply)? = Some(document.clone());
                return Err(CentralAutomationError::OutcomeUnknown);
            }
            Ok(answer)
        })
    }
}

#[tokio::test]
async fn applied_central_write_with_lost_answer_keeps_operation_identity_and_replays_once()
-> ProofResult {
    // Arrange: only the carrier drops an answer, after real central has returned its commit.
    let evidence = Arc::new(LostReplyEvidence::default());
    let captured = evidence.clone();
    let fixture = CatalogFixture::open(
        move |inner| {
            Arc::new(LostInstructionReply {
                inner,
                evidence: captured,
            })
        },
        false,
    )
    .await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("catalog-write-loss").await?;
        let request = InstructionCreateParams { operation_id: OperationId::generate(), text: "Committed before its answer is dropped".to_owned().try_into()? };
        // Act: the public API receives an unknown outcome, never definite non-submission.
        let failed = client.create_instruction(request.clone()).await;
        if !matches!(failed, Err(InstructionClientError::Catalog(CentralCatalogFailure::OutcomeUnknown { operation_id, .. })) if operation_id == request.operation_id) {
            return Err("lost applied answer did not preserve unknown outcome and caller operation ID".into());
        }
        if evidence.calls.load(Ordering::SeqCst) != 1 {
            return Err("catalog adapter automatically retried an unknown write".into());
        }
        let committed = evidence.committed.lock().map_err(|_| "commit evidence poisoned")?.clone().ok_or("central never returned its actual commit")?;
        // Assert: only an explicit same-ID replay recovers that exact instruction/revision.
        let replay = client.create_instruction(request.clone()).await?;
        if serde_json::to_value(replay)? != serde_json::to_value(&committed)? {
            return Err("same-operation replay allocated another instruction or revision".into());
        }
        let changed = client.create_instruction(InstructionCreateParams {
            text: "Same operation with changed content".to_owned().try_into()?, ..request.clone()
        }).await;
        if !matches!(changed, Err(InstructionClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == request.operation_id && matches!(&*error, AutomationRefusal::OperationConflict { operation_id } if operation_id == &request.operation_id)) {
            return Err("changed-content replay lost central's operation conflict".into());
        }
        if evidence.calls.load(Ordering::SeqCst) != 3 {
            return Err("write count differs from the three explicit caller requests".into());
        }
        let AutomationAnswer::Instructions(page) = fixture.transport.call(AutomationCall::InstructionList(AutomationPageRequest { cursor: None, limit: 100.try_into()? })).await? else {
            return Err("actual central instruction page missing".into());
        };
        if page.records.len() != 1 || page.records[0].instruction_id != committed.instruction_id || page.next_cursor.is_some() {
            return Err("unknown outcome or replay duplicated the central write".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn catalog_page_budget_keeps_complete_rows_and_central_continuation() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, false).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("catalog-page-budget").await?;
        for prefix in ["first", "other"] {
            client.create_instruction(InstructionCreateParams {
                operation_id: OperationId::generate(), text: format!("{prefix}{}", "\\\"".repeat(512)).try_into()?,
            }).await?;
        }
        let request = AutomationPageRequest { cursor: None, limit: 1.try_into()? };
        let AutomationAnswer::Instructions(expected) = fixture.transport.call(AutomationCall::InstructionList(request.clone())).await? else {
            return Err("actual central page missing".into());
        };
        let bytes = serde_json::to_vec(&expected)?.len();
        let budget = collaboration_service::ResultByteBudget::new(bytes, 0);
        // Act: a larger requested page must shrink at central, preserving its real cursor.
        let page = fixture.application.automation().instruction_list(AutomationPageRequest { limit: 100.try_into()?, ..request.clone() }, budget).await?;
        if serde_json::to_value(&page)? != serde_json::to_value(&expected)? || !budget.admits(&page) {
            return Err("budget adaptation changed a record or manufactured a continuation".into());
        }
        let next = fixture.application.automation().instruction_list(AutomationPageRequest { cursor: page.next_cursor.clone(), limit: 100.try_into()? }, budget).await?;
        if next.records.len() != 1 || next.records[0].instruction_id == page.records[0].instruction_id || next.next_cursor.is_some() {
            return Err("bounded page continuation lost or repeated a central document".into());
        }
        // Assert: one byte below a single complete page refuses it without truncating text.
        let too_small = fixture.application.automation().instruction_list(request, collaboration_service::ResultByteBudget::new(bytes - 1, 0)).await;
        if !matches!(too_small, Err(collaboration_service::CatalogPageFailure::Inspection(error)) if matches!(error.kind, collaboration_protocol::AutomationInspectionFailureKind::InvalidField) && error.field.as_deref() == Some("limit")) {
            return Err("an oversized single record was truncated or escaped its budget".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn missing_central_refuses_before_local_definition_or_operation_is_written() -> ProofResult {
    use sqlx::Connection;
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::Builder::new()
        .prefix("s6cat-offline-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = directory.path().join("automation.sqlite");
    let store = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&path).await?,
    ));
    let application = collaboration_service::ApplicationFixture::new(
        fixture::SERVICE,
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_automation_store(store.clone())
    .build();
    let api = collaboration_mcp::test_support::ServedCollaborationApi::start(
        directory.path(),
        application,
    )?;
    let proof: ProofResult = async {
        let client = api.client("catalog-unsent").await?;
        let operation = OperationId::generate();
        let refused = client.create_instruction(InstructionCreateParams {
            operation_id: operation.clone(), text: "Must never become a local write".to_owned().try_into()?,
        }).await;
        if !matches!(refused, Err(InstructionClientError::Catalog(CentralCatalogFailure::CentralUnreachable { operation_id: Some(actual), .. })) if actual == operation) {
            return Err("absent central did not return definite unsent with the caller operation ID".into());
        }
        let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await?;
        let definitions: i64 = sqlx::query_scalar("SELECT count(*) FROM instruction_documents").fetch_one(&mut sql).await?;
        let operations: i64 = sqlx::query_scalar("SELECT count(*) FROM operation_receipts").fetch_one(&mut sql).await?;
        sql.close().await?;
        if definitions != 0 || operations != 0 {
            return Err("unreachable central silently queued or committed a local catalog mutation".into());
        }
        Ok(())
    }.await;
    let stopped = api.stop().await;
    drop(store);
    let removed = directory.close();
    proof?;
    stopped?;
    removed?;
    Ok(())
}
