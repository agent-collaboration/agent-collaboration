use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, NonEmptyText, OperationId,
    ProviderBindingId, ProviderBindingIdentity, ProviderCapabilities, ProviderCapability,
    ProviderCapabilityEvidence, ProviderCapabilityName, ProviderCapabilityStatus, ProviderIdentity,
    ProviderKind, ProviderOperationEffect, ProviderOperationKind, ProviderOperationStage,
    ProviderRequestedPolicy, ProviderRuntimeIdentity, ProviderTransport, ProviderWorkingDirectory,
    RouterAccess, SessionId, SessionRef, UuidIdentity,
};
use collaboration_runtime::ProviderOperationAdmission;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderOperationStoreError;
use collaboration_runtime::ProviderSessionRecord;
use collaboration_runtime::ProviderSessionResumeInputs;
use message_board::HumanId;
use sqlx::{Connection, SqliteConnection};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

macro_rules! ensure {
    ($condition:expr) => {
        if !$condition {
            return Err(format!("assertion failed: {}", stringify!($condition)).into());
        }
    };
}

macro_rules! ensure_eq {
    ($left:expr, $right:expr) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!(
                "assertion failed: {} == {}: left={left:?}, right={right:?}",
                stringify!($left),
                stringify!($right)
            )
            .into());
        }
    }};
}

fn session_ref(endpoint_id: &str, session_id: &str) -> TestResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())?,
            endpoint_id: EndpointId::try_from(endpoint_id.to_owned())?,
        },
        session_id: SessionId::try_from(session_id.to_owned())?,
    })
}

fn provider_binding(target: &SessionRef) -> TestResult<ProviderBindingIdentity> {
    Ok(ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("provider-binding-1".to_owned())?,
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::ClaudeCode,
            runtime_name: NonEmptyText::try_from("fixture-provider".to_owned())?,
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: UuidIdentity::try_from(
                "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80".to_owned(),
            )?,
            generation: GenerationNumber::try_from(1)?,
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Create,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])?,
    })
}

#[tokio::test]
async fn provider_session_metadata_round_trips_and_rejects_invalid_stored_rows() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("provider-operations.sqlite");
    let mut store = ProviderOperationStore::open(&path).await?;
    let target = session_ref("claude-local", "provider-session-1")?;
    let record = ProviderSessionRecord {
        target: target.clone(),
        working_directory: ProviderWorkingDirectory::try_from(root.path().display().to_string())?,
        requested_policy: ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        },
        created_by: session_ref("codex-local", "creator-1")?.into(),
        approver: session_ref("codex-local", "approver-1")?.into(),
        updated_at_ms: 10,
        resume: Default::default(),
    };

    store.record_session(&record).await?;
    let restored = store
        .session_record(&target)
        .await?
        .ok_or("session record missing")?;
    ensure_eq!(restored, record);

    let mut raw = SqliteConnection::connect(&format!("sqlite:{}", path.display())).await?;
    let stored_creator: String = sqlx::query_scalar(
        "SELECT created_by_json FROM provider_session_records WHERE target_session_id = 'provider-session-1'",
    )
    .fetch_one(&mut raw)
    .await?;
    ensure_eq!(stored_creator, serde_json::to_string(&record.created_by)?);
    ensure_eq!(
        stored_creator,
        serde_json::to_string(&session_ref("codex-local", "creator-1")?)?
    );

    sqlx::query("UPDATE provider_session_records SET working_directory = 'relative' WHERE target_session_id = 'provider-session-1'")
        .execute(&mut raw)
        .await?;
    raw.close().await?;
    ensure!(matches!(
        store.session_record(&target).await,
        Err(ProviderOperationStoreError::InvalidRecord)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn human_creator_and_approver_survive_store_reopen_and_old_reader_fails_closed() -> TestResult
{
    let root = tempfile::tempdir()?;
    let path = root.path().join("provider-operations.sqlite");
    let target = session_ref("claude-local", "human-owned-session")?;
    let human = ProviderIdentity::Human {
        human_id: HumanId::try_from("owner".to_owned())?,
    };
    let record = ProviderSessionRecord {
        target: target.clone(),
        working_directory: ProviderWorkingDirectory::try_from(root.path().display().to_string())?,
        requested_policy: ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        },
        created_by: human.clone(),
        approver: human.clone(),
        updated_at_ms: 10,
        resume: Default::default(),
    };
    let mut store = ProviderOperationStore::open(&path).await?;
    store.record_session(&record).await?;
    store.close().await?;

    let mut reopened = ProviderOperationStore::open(&path).await?;
    ensure_eq!(reopened.session_record(&target).await?, Some(record));
    reopened.close().await?;

    let mut raw = SqliteConnection::connect(&format!("sqlite:{}", path.display())).await?;
    let stored_creator: String = sqlx::query_scalar(
        "SELECT created_by_json FROM provider_session_records WHERE target_session_id = 'human-owned-session'",
    )
    .fetch_one(&mut raw)
    .await?;
    ensure_eq!(stored_creator, r#"{"humanId":"owner"}"#);
    let old_reader = serde_json::from_str::<SessionRef>(&stored_creator)
        .expect_err("old SessionRef-only reader must reject a human creator");
    ensure!(old_reader.to_string().contains("humanId"));
    raw.close().await?;
    Ok(())
}

#[tokio::test]
async fn create_settlement_and_session_record_commit_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("provider-operations.sqlite");
    let mut store = ProviderOperationStore::open(&path).await?;
    let target = session_ref("claude-local", "provider-session-2")?;
    let operation_id = OperationId::try_from("018f1f62-6571-7ef0-8f0c-001122334488".to_owned())?;
    store
        .admit(ProviderOperationAdmission {
            operation_id: operation_id.clone(),
            operation_kind: ProviderOperationKind::ConversationCreate,
            binding: collaboration_protocol::ConversationBindingIdentity::ExternalProvider {
                binding: provider_binding(&target)?,
            },
            admitted_at_ms: 10,
        })
        .await?;
    store.mark_may_have_dispatched(&operation_id, 11).await?;
    let mut record = ProviderSessionRecord {
        target: session_ref("cursor-local", "wrong-endpoint")?,
        working_directory: ProviderWorkingDirectory::try_from(root.path().display().to_string())?,
        requested_policy: ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        },
        created_by: session_ref("codex-local", "creator-2")?.into(),
        approver: session_ref("codex-local", "approver-2")?.into(),
        updated_at_ms: 12,
        resume: Default::default(),
    };

    ensure!(matches!(
        store.settle_session_operation(&operation_id, &record).await,
        Err(ProviderOperationStoreError::InvalidRecord)
    ));
    ensure_eq!(
        store
            .inspect(&operation_id)
            .await?
            .ok_or("operation missing")?
            .stage,
        ProviderOperationStage::MayHaveDispatched
    );
    record.target = target.clone();
    store
        .settle_session_operation(&operation_id, &record)
        .await?;

    let settled = store
        .inspect(&operation_id)
        .await?
        .ok_or("settled operation missing")?;
    ensure_eq!(settled.stage, ProviderOperationStage::Terminal);
    ensure_eq!(settled.effect, ProviderOperationEffect::Applied);
    ensure_eq!(settled.target, Some(target.clone()));
    ensure_eq!(store.session_record(&target).await?, Some(record));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn provider_session_inventory_lists_only_the_requested_endpoint() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite")).await?;
    for (endpoint_id, session_id, updated_at_ms) in [
        ("claude-local", "older", 10),
        ("cursor-local", "other-endpoint", 30),
        ("claude-local", "newer", 20),
    ] {
        let target = session_ref(endpoint_id, session_id)?;
        store
            .record_session(&ProviderSessionRecord {
                target,
                working_directory: ProviderWorkingDirectory::try_from(
                    root.path().display().to_string(),
                )?,
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: session_ref("codex-local", "creator")?.into(),
                approver: session_ref("codex-local", "approver")?.into(),
                updated_at_ms,
                resume: Default::default(),
            })
            .await?;
    }
    let endpoint = session_ref("claude-local", "unused")?.endpoint;
    let records = store.list_sessions(&endpoint).await?;
    ensure_eq!(records.len(), 2);
    ensure_eq!(String::from(records[0].target.session_id.clone()), "newer");
    ensure_eq!(String::from(records[1].target.session_id.clone()), "older");
    store.close().await?;
    Ok(())
}

/// A record for `target` with `resume`, working in `root`.
fn record_with_resume(
    root: &std::path::Path,
    target: &SessionRef,
    resume: ProviderSessionResumeInputs,
    updated_at_ms: i64,
) -> TestResult<ProviderSessionRecord> {
    Ok(ProviderSessionRecord {
        target: target.clone(),
        working_directory: ProviderWorkingDirectory::try_from(root.display().to_string())?,
        requested_policy: ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        },
        created_by: session_ref("codex-local", "creator")?.into(),
        approver: session_ref("codex-local", "approver")?.into(),
        updated_at_ms,
        resume,
    })
}

/// Oracle: owner Decision 3 (option A), "write at create, read at resume to
/// rebuild the roots and the mode": what a create recorded (its requested
/// mode, its creator-named roots, its scratch folder) reads back after the
/// store is reopened, and a later load's or resume's settlement, which writes
/// none of them, keeps them. The inventory read answers the same.
#[tokio::test]
async fn resume_inputs_survive_a_reopen_and_a_later_settlement_that_writes_none() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let path = root.path().join("provider-operations.sqlite");
    let target = session_ref("claude-local", "resumable-session")?;
    let named_root = root.path().join("named-root");
    let scratch = root.path().join("scratch/session-1");
    let created = record_with_resume(
        root.path(),
        &target,
        ProviderSessionResumeInputs {
            requested_mode: Some("auto".to_owned()),
            work_roots: Some(vec![ProviderWorkingDirectory::try_from(
                named_root.display().to_string(),
            )?]),
            scratch_folder: Some(ProviderWorkingDirectory::try_from(
                scratch.display().to_string(),
            )?),
        },
        10,
    )?;
    let mut store = ProviderOperationStore::open(&path).await?;
    store.record_session(&created).await?;
    store.close().await?;

    // Act
    let mut reopened = ProviderOperationStore::open(&path).await?;
    let after_reopen = reopened.session_record(&target).await?;
    let loaded = record_with_resume(
        root.path(),
        &target,
        ProviderSessionResumeInputs::default(),
        20,
    )?;
    reopened.record_session(&loaded).await?;
    let after_load = reopened.session_record(&target).await?;
    let listed = reopened.list_sessions(&target.endpoint).await?;
    reopened.close().await?;

    // Assert
    ensure_eq!(after_reopen, Some(created.clone()));
    let after_load = after_load.ok_or("session record missing after the load")?;
    ensure_eq!(after_load.updated_at_ms, 20);
    ensure_eq!(after_load.resume, created.resume);
    ensure_eq!(listed.len(), 1);
    ensure_eq!(listed[0].resume, created.resume);
    Ok(())
}

/// Oracle: owner Decision 3, "old rows keep the fallback": a row written
/// before the resume inputs existed (none of the new columns set) reads with
/// none of them, so a resume of it rebuilds nothing.
#[tokio::test]
async fn a_row_written_without_resume_inputs_reads_none() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let path = root.path().join("provider-operations.sqlite");
    let target = session_ref("claude-local", "old-session")?;
    let mut store = ProviderOperationStore::open(&path).await?;
    let mut raw = SqliteConnection::connect(&format!("sqlite:{}", path.display())).await?;
    sqlx::query(
        "INSERT INTO provider_session_records (
            target_service_id,target_endpoint_id,target_session_id,
            working_directory,requested_policy_json,created_by_json,approver_json,updated_at_ms
        ) VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(String::from(target.endpoint.service_id.clone()))
    .bind(String::from(target.endpoint.endpoint_id.clone()))
    .bind(String::from(target.session_id.clone()))
    .bind(root.path().display().to_string())
    .bind(serde_json::to_string(&ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    })?)
    .bind(serde_json::to_string(&session_ref(
        "codex-local",
        "creator",
    )?)?)
    .bind(serde_json::to_string(&session_ref(
        "codex-local",
        "approver",
    )?)?)
    .bind(10_i64)
    .execute(&mut raw)
    .await?;
    raw.close().await?;

    // Act
    let restored = store.session_record(&target).await?;
    store.close().await?;

    // Assert
    let restored = restored.ok_or("session record missing")?;
    ensure_eq!(restored.resume, ProviderSessionResumeInputs::default());
    Ok(())
}

/// Oracle: AGENTS.md, "Reject invalid stored values explicitly; never
/// silently coerce them": a stored roots list that isn't JSON, a root or a
/// scratch folder that isn't an absolute path, each makes the record read
/// fail, by target and in the inventory.
#[tokio::test]
async fn invalid_stored_resume_inputs_are_rejected() -> TestResult {
    const SET_WORK_ROOTS: &str = "UPDATE provider_session_records SET work_roots_json = ? WHERE target_session_id = 'corrupt-session'";
    const SET_SCRATCH: &str = "UPDATE provider_session_records SET scratch_folder = ? WHERE target_session_id = 'corrupt-session'";
    for (update, stored) in [
        (SET_WORK_ROOTS, "not json"),
        (SET_WORK_ROOTS, r#"["relative"]"#),
        (SET_SCRATCH, "relative"),
    ] {
        // Arrange
        let root = tempfile::tempdir()?;
        let path = root.path().join("provider-operations.sqlite");
        let target = session_ref("claude-local", "corrupt-session")?;
        let mut store = ProviderOperationStore::open(&path).await?;
        store
            .record_session(&record_with_resume(
                root.path(),
                &target,
                ProviderSessionResumeInputs::default(),
                10,
            )?)
            .await?;
        let mut raw = SqliteConnection::connect(&format!("sqlite:{}", path.display())).await?;
        sqlx::query(update).bind(stored).execute(&mut raw).await?;
        raw.close().await?;

        // Act
        let by_target = store.session_record(&target).await;
        let listed = store.list_sessions(&target.endpoint).await;
        store.close().await?;

        // Assert
        ensure!(matches!(
            by_target,
            Err(ProviderOperationStoreError::InvalidRecord)
        ));
        ensure!(matches!(
            listed,
            Err(ProviderOperationStoreError::InvalidRecord)
        ));
    }
    Ok(())
}
