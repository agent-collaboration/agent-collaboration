//! Durable provider session metadata needed for load-on-demand and approvals.
use crate::provider_operation_store::OneStatementWrite;
use crate::{ProviderOperationStore, ProviderOperationStoreError};
use collaboration_protocol::{
    ConversationBindingIdentity, EndpointRef, OperationId, ProviderIdentity,
    ProviderOperationEffect, ProviderOperationKind, ProviderOperationStage,
    ProviderReconciliationState, ProviderRequestedPolicy, ProviderWorkingDirectory, SessionId,
    SessionRef,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSessionRecord {
    pub target: SessionRef,
    pub working_directory: ProviderWorkingDirectory,
    pub requested_policy: ProviderRequestedPolicy,
    pub created_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub updated_at_ms: i64,
    /// What a resume needs to rebuild the session's setup.
    pub resume: ProviderSessionResumeInputs,
}

/// What a resumed session needs to rebuild the setup it was created with
/// (D1 R18; owner Decision 3, option A): the mode it was asked to start in,
/// the work roots its creator named, and its scratch folder. A session
/// recorded before these were kept has none of them, and a resume keeps the
/// fallback: no Router allowance, every request asks the approver.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderSessionResumeInputs {
    pub requested_mode: Option<String>,
    pub work_roots: Option<Vec<ProviderWorkingDirectory>>,
    pub scratch_folder: Option<ProviderWorkingDirectory>,
}

/// The three resume inputs as their stored column values.
struct StoredResumeInputs {
    requested_mode: Option<String>,
    work_roots_json: Option<String>,
    scratch_folder: Option<String>,
}

impl ProviderSessionResumeInputs {
    /// The requested mode is stored as the caller gave it (the protocol puts
    /// no rule on it); the folders are validated paths.
    fn encode(&self) -> Result<StoredResumeInputs, ProviderOperationStoreError> {
        let work_roots_json = self
            .work_roots
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        Ok(StoredResumeInputs {
            requested_mode: self.requested_mode.clone(),
            work_roots_json,
            scratch_folder: self
                .scratch_folder
                .as_ref()
                .map(|folder| String::from(folder.clone())),
        })
    }

    /// Decodes stored columns; an invalid stored value is rejected, never
    /// coerced.
    fn decode(stored: StoredResumeInputs) -> Result<Self, ProviderOperationStoreError> {
        let work_roots = stored
            .work_roots_json
            .map(|json| {
                serde_json::from_str::<Vec<ProviderWorkingDirectory>>(&json)
                    .map_err(|_| ProviderOperationStoreError::InvalidRecord)
            })
            .transpose()?;
        let scratch_folder = stored
            .scratch_folder
            .map(|folder| {
                ProviderWorkingDirectory::try_from(folder)
                    .map_err(|_| ProviderOperationStoreError::InvalidRecord)
            })
            .transpose()?;
        Ok(Self {
            requested_mode: stored.requested_mode,
            work_roots,
            scratch_folder,
        })
    }
}

impl ProviderSessionRecord {
    fn validate(&self) -> Result<(), ProviderOperationStoreError> {
        let endpoint_id = String::from(self.target.endpoint.endpoint_id.clone());
        if !matches!(endpoint_id.as_str(), "claude-local" | "cursor-local")
            || self.created_by.session().is_some_and(|session| {
                session.endpoint.service_id != self.target.endpoint.service_id
            })
            || self.approver.session().is_some_and(|session| {
                session.endpoint.service_id != self.target.endpoint.service_id
            })
            || self.updated_at_ms < 0
        {
            return Err(ProviderOperationStoreError::InvalidRecord);
        }
        Ok(())
    }
}

impl ProviderOperationStore {
    /// Atomically settle create/load/resume and make its target loadable after a restart.
    pub async fn settle_session_operation(
        &mut self,
        operation_id: &OperationId,
        record: &ProviderSessionRecord,
    ) -> Result<(), ProviderOperationStoreError> {
        record.validate()?;
        let operation = self
            .inspect(operation_id)
            .await?
            .ok_or(ProviderOperationStoreError::NotFound)?;
        let ConversationBindingIdentity::ExternalProvider { binding } = &operation.binding else {
            return Err(ProviderOperationStoreError::InvalidRecord);
        };
        if binding.endpoint != record.target.endpoint {
            return Err(ProviderOperationStoreError::InvalidRecord);
        }
        let target_service_id = String::from(record.target.endpoint.service_id.clone());
        let target_endpoint_id = String::from(record.target.endpoint.endpoint_id.clone());
        let target_session_id = String::from(record.target.session_id.clone());
        let working_directory = String::from(record.working_directory.clone());
        let requested_policy_json = serde_json::to_string(&record.requested_policy)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let created_by_json = serde_json::to_string(&record.created_by)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let approver_json = serde_json::to_string(&record.approver)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let resume = record.resume.encode()?;
        let mut transaction = self
            .transaction(sqlite_storage_maintenance::TransactionStart::Immediate)
            .await?;
        let updated = sqlx::query!(
            "UPDATE provider_operations SET
                target_service_id=?,target_endpoint_id=?,target_session_id=?,
                stage=?,effect=?,reconciliation_state=?,
                terminal_at_ms=MAX(admitted_at_ms,?),
                updated_at_ms=MAX(updated_at_ms,admitted_at_ms,?)
             WHERE operation_id=? AND stage=? AND operation_kind IN (?,?,?)
             AND (target_session_id IS NULL OR
                 (target_service_id=? AND target_endpoint_id=? AND target_session_id=?))",
            target_service_id,
            target_endpoint_id,
            target_session_id,
            super::provider_operation_store::encode_enum(ProviderOperationStage::Terminal)?,
            super::provider_operation_store::encode_enum(ProviderOperationEffect::Applied)?,
            super::provider_operation_store::encode_enum(ProviderReconciliationState::Confirmed)?,
            record.updated_at_ms,
            record.updated_at_ms,
            operation_id.as_str(),
            super::provider_operation_store::encode_enum(
                ProviderOperationStage::MayHaveDispatched
            )?,
            super::provider_operation_store::encode_enum(
                ProviderOperationKind::ConversationCreate
            )?,
            super::provider_operation_store::encode_enum(ProviderOperationKind::ConversationLoad)?,
            super::provider_operation_store::encode_enum(
                ProviderOperationKind::ConversationResume
            )?,
            target_service_id,
            target_endpoint_id,
            target_session_id,
        )
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(ProviderOperationStoreError::TransitionConflict);
        }
        sqlx::query!(
            "INSERT INTO provider_session_records (
                target_service_id,target_endpoint_id,target_session_id,
                working_directory,requested_policy_json,created_by_json,approver_json,updated_at_ms,
                requested_mode,work_roots_json,scratch_folder
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(target_service_id,target_endpoint_id,target_session_id) DO UPDATE SET
                working_directory=excluded.working_directory,
                requested_policy_json=excluded.requested_policy_json,
                approver_json=excluded.approver_json,
                updated_at_ms=excluded.updated_at_ms,
                requested_mode=COALESCE(excluded.requested_mode,provider_session_records.requested_mode),
                work_roots_json=COALESCE(excluded.work_roots_json,provider_session_records.work_roots_json),
                scratch_folder=COALESCE(excluded.scratch_folder,provider_session_records.scratch_folder)",
            target_service_id,
            target_endpoint_id,
            target_session_id,
            working_directory,
            requested_policy_json,
            created_by_json,
            approver_json,
            record.updated_at_ms,
            resume.requested_mode,
            resume.work_roots_json,
            resume.scratch_folder,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record_session(
        &mut self,
        record: &ProviderSessionRecord,
    ) -> Result<(), ProviderOperationStoreError> {
        record.validate()?;
        let target_service_id = String::from(record.target.endpoint.service_id.clone());
        let target_endpoint_id = String::from(record.target.endpoint.endpoint_id.clone());
        let target_session_id = String::from(record.target.session_id.clone());
        let working_directory = String::from(record.working_directory.clone());
        let requested_policy_json = serde_json::to_string(&record.requested_policy)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let created_by_json = serde_json::to_string(&record.created_by)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let approver_json = serde_json::to_string(&record.approver)
            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
        let resume = record.resume.encode()?;
        sqlx::query!(
            "INSERT INTO provider_session_records (
                target_service_id,target_endpoint_id,target_session_id,
                working_directory,requested_policy_json,created_by_json,approver_json,updated_at_ms,
                requested_mode,work_roots_json,scratch_folder
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(target_service_id,target_endpoint_id,target_session_id) DO UPDATE SET
                working_directory=excluded.working_directory,
                requested_policy_json=excluded.requested_policy_json,
                approver_json=excluded.approver_json,
                updated_at_ms=excluded.updated_at_ms,
                requested_mode=COALESCE(excluded.requested_mode,provider_session_records.requested_mode),
                work_roots_json=COALESCE(excluded.work_roots_json,provider_session_records.work_roots_json),
                scratch_folder=COALESCE(excluded.scratch_folder,provider_session_records.scratch_folder)",
            target_service_id,
            target_endpoint_id,
            target_session_id,
            working_directory,
            requested_policy_json,
            created_by_json,
            approver_json,
            record.updated_at_ms,
            resume.requested_mode,
            resume.work_roots_json,
            resume.scratch_folder,
        )
        .execute_as_transaction(self)
        .await?;
        Ok(())
    }

    pub async fn session_record(
        &mut self,
        target: &SessionRef,
    ) -> Result<Option<ProviderSessionRecord>, ProviderOperationStoreError> {
        let target_service_id = String::from(target.endpoint.service_id.clone());
        let target_endpoint_id = String::from(target.endpoint.endpoint_id.clone());
        let target_session_id = String::from(target.session_id.clone());
        let row = sqlx::query!(
            "SELECT working_directory,requested_policy_json,created_by_json,approver_json,updated_at_ms,
                    requested_mode,work_roots_json,scratch_folder
             FROM provider_session_records
             WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=?",
            target_service_id,
            target_endpoint_id,
            target_session_id,
        )
        .fetch_optional(&mut self.connection)
        .await?;
        row.map(|row| {
            let working_directory = ProviderWorkingDirectory::try_from(row.working_directory)
                .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
            let requested_policy = serde_json::from_str(&row.requested_policy_json)
                .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
            let created_by = serde_json::from_str(&row.created_by_json)
                .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
            let approver = serde_json::from_str(&row.approver_json)
                .map_err(|_| ProviderOperationStoreError::InvalidRecord)?;
            let record = ProviderSessionRecord {
                target: target.clone(),
                working_directory,
                requested_policy,
                created_by,
                approver,
                updated_at_ms: row.updated_at_ms,
                resume: ProviderSessionResumeInputs::decode(StoredResumeInputs {
                    requested_mode: row.requested_mode,
                    work_roots_json: row.work_roots_json,
                    scratch_folder: row.scratch_folder,
                })?,
            };
            record.validate()?;
            Ok(record)
        })
        .transpose()
    }

    /// The durable provider inventory is the source for every front door's
    /// session list. Hub live state is overlaid after this read.
    pub async fn list_sessions(
        &mut self,
        endpoint: &EndpointRef,
    ) -> Result<Vec<ProviderSessionRecord>, ProviderOperationStoreError> {
        let target_service_id = String::from(endpoint.service_id.clone());
        let target_endpoint_id = String::from(endpoint.endpoint_id.clone());
        let rows = sqlx::query!(
            "SELECT target_session_id,working_directory,requested_policy_json,
                    created_by_json,approver_json,updated_at_ms,
                    requested_mode,work_roots_json,scratch_folder
             FROM provider_session_records
             WHERE target_service_id=? AND target_endpoint_id=?
             ORDER BY updated_at_ms DESC,target_session_id ASC",
            target_service_id,
            target_endpoint_id,
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let target = SessionRef {
                    endpoint: endpoint.clone(),
                    session_id: SessionId::try_from(row.target_session_id)
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                };
                let record = ProviderSessionRecord {
                    target,
                    working_directory: ProviderWorkingDirectory::try_from(row.working_directory)
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                    requested_policy: serde_json::from_str(&row.requested_policy_json)
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                    created_by: serde_json::from_str(&row.created_by_json)
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                    approver: serde_json::from_str(&row.approver_json)
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                    updated_at_ms: row.updated_at_ms,
                    resume: ProviderSessionResumeInputs::decode(StoredResumeInputs {
                        requested_mode: row.requested_mode,
                        work_roots_json: row.work_roots_json,
                        scratch_folder: row.scratch_folder,
                    })?,
                };
                record.validate()?;
                Ok(record)
            })
            .collect()
    }
}
