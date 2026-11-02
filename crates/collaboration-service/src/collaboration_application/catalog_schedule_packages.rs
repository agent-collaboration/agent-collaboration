//! Portable text is validated locally; catalog import owns the only definition writes.
use super::automation_operations::AutomationOperations;
use crate::schedule_command_failure::{ScheduleFailureContext, schedule_failure};
use crate::{
    CentralCatalogFailure,
    catalog_read_failure::CatalogReadFailure,
    catalog_results::CatalogMutationResult,
    collaboration_rejection::{
        CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
    },
};
use agent_automation::{PackageError, PortableSchedulePackage};
use collaboration_protocol::{
    AutomationCall, EndpointRef, LocalMutationState, ScheduleFailure, ScheduleImportRequest,
    SchedulePackage, SchedulePackageContinuity, SchedulePackageImportRequest,
    SchedulePackageInstruction, SessionRef,
};

#[derive(Debug, thiserror::Error)]
pub enum SchedulePackageFailure {
    #[error(transparent)]
    Command(Box<crate::ScheduleCommandFailure>),
    #[error(transparent)]
    Catalog(Box<CentralCatalogFailure>),
    #[error("Schedule package transfer failed")]
    Package(Box<ScheduleFailure>),
    #[error(transparent)]
    ReadFailure(Box<CatalogReadFailure>),
}
impl From<crate::ScheduleCommandFailure> for SchedulePackageFailure {
    fn from(value: crate::ScheduleCommandFailure) -> Self {
        Self::Command(Box::new(value))
    }
}
impl From<CentralCatalogFailure> for SchedulePackageFailure {
    fn from(error: CentralCatalogFailure) -> Self {
        Self::Catalog(Box::new(error))
    }
}
impl CollaborationRejection for SchedulePackageFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::Command(error) => error.rejection_reason(),
            Self::Catalog(error) => error.rejection_reason(),
            Self::Package(error) => error.rejection_reason(),
            Self::ReadFailure(error) => error.rejection_reason(),
        }
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Command(error) => error.published_rejection(),
            Self::Catalog(error) => error.published_rejection(),
            Self::ReadFailure(error) => error.published_rejection(),
            Self::Package(error) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                self.to_string(),
                error,
            ),
        }
    }
}

impl AutomationOperations {
    /// Reads central definition truth and this target machine's finished Run continuity.
    pub async fn schedule_export(
        &self,
        request: collaboration_protocol::ScheduleShowRequest,
        budget: super::ResultByteBudget,
    ) -> Result<collaboration_protocol::ScheduleExportResult, SchedulePackageFailure> {
        let context = ScheduleFailureContext::new(None, Some(request.schedule_id.clone()));
        let failure = |error| {
            SchedulePackageFailure::Package(Box::new(schedule_failure(
                context.clone(),
                error,
                LocalMutationState::None,
                None,
            )))
        };
        let schedule = match self.schedule_show(request).await? {
            crate::catalog_results::ScheduleReadResponse::Suspended { suspended, .. } => {
                return Err(SchedulePackageFailure::ReadFailure(Box::new(
                    CatalogReadFailure::suspended(suspended),
                )));
            }
            crate::catalog_results::ScheduleReadResponse::Schedule { value } => value,
            crate::catalog_results::ScheduleReadResponse::Local { value } => {
                let package = self
                    .store()
                    .ok_or_else(|| failure(automation_storage::StorageError::InvalidRecord))?
                    .lock()
                    .await
                    .export_schedule::<SessionRef, EndpointRef>(&value.schedule_id)
                    .await
                    .map_err(failure)?;
                return encode_export(package, budget, context);
            }
            crate::catalog_results::ScheduleReadResponse::Faulted { fault, .. } => {
                return Err(SchedulePackageFailure::ReadFailure(Box::new(
                    CatalogReadFailure::faulted(fault),
                )));
            }
        };
        let instruction = self
            .instruction_show(collaboration_protocol::InstructionShowParams {
                instruction_id: schedule.definition.instruction_id.clone(),
            })
            .await?;
        let imported: agent_automation::ContinuityInput<SessionRef> = serde_json::from_value(
            serde_json::to_value(&schedule.imported_continuity)
                .map_err(|_| failure(automation_storage::StorageError::InvalidRecord))?,
        )
        .map_err(|_| failure(automation_storage::StorageError::InvalidRecord))?;
        let target_here = match &schedule.definition.destination {
            collaboration_protocol::ExecutionDestination::OwnedThread { target, .. } => {
                target.endpoint.service_id == *self.identity.service_id()
            }
            collaboration_protocol::ExecutionDestination::FreshEachRun { endpoint, .. } => {
                endpoint.service_id == *self.identity.service_id()
            }
            collaboration_protocol::ExecutionDestination::Unprepared
            | collaboration_protocol::ExecutionDestination::FreshEachRunUnprepared => false,
        };
        let continuity = if target_here {
            self.store()
                .ok_or_else(|| failure(automation_storage::StorageError::InvalidRecord))?
                .lock()
                .await
                .export_schedule_continuity(&schedule.schedule_id, imported)
                .await
                .map_err(failure)?
        } else {
            match imported {
                agent_automation::ContinuityInput::None => None,
                agent_automation::ContinuityInput::ImportedSummary {
                    text,
                    source_run_id,
                    source_target,
                    ..
                } => Some(agent_automation::PortableContinuity {
                    text: text
                        .try_into()
                        .map_err(|_| failure(automation_storage::StorageError::InvalidRecord))?,
                    source_run_id,
                    source_target,
                }),
                _ => return Err(failure(automation_storage::StorageError::InvalidRecord)),
            }
        };
        let package = PortableSchedulePackage::<SessionRef, EndpointRef> {
            schedule_id: schedule.schedule_id,
            source_change_id: schedule.change_id.as_str().into(),
            instruction: agent_automation::PortableInstruction {
                instruction_id: instruction.instruction_id,
                text: instruction.text,
                source_revision_id: instruction.revision_id.as_str().into(),
            },
            definition: serde_json::from_value(
                serde_json::to_value(schedule.definition)
                    .map_err(|_| failure(automation_storage::StorageError::InvalidRecord))?,
            )
            .map_err(|_| failure(automation_storage::StorageError::InvalidRecord))?,
            continuity,
        };
        encode_export(package, budget, context)
    }

    pub async fn schedule_import(
        &self,
        request: ScheduleImportRequest,
    ) -> Result<CatalogMutationResult, SchedulePackageFailure> {
        let package = agent_automation::decode_schedule_package::<SessionRef, EndpointRef>(
            &request.package_utf8,
            collaboration_protocol::MAX_CONTROL_FRAME_BYTES,
        )
        .and_then(catalog_package)
        .map_err(|error| {
            SchedulePackageFailure::Package(Box::new(schedule_failure(
                ScheduleFailureContext::new(Some(request.operation_id.clone()), None),
                error.into(),
                LocalMutationState::None,
                None,
            )))
        })?;
        self.change_schedule(
            request.operation_id.clone(),
            package.schedule_id.clone(),
            AutomationCall::SchedulePackageImport(SchedulePackageImportRequest {
                operation_id: request.operation_id,
                package,
                overwrite: request.overwrite,
            }),
        )
        .await
        .map_err(Into::into)
    }
}

fn catalog_package(
    package: PortableSchedulePackage<SessionRef, EndpointRef>,
) -> Result<SchedulePackage, PackageError> {
    Ok(SchedulePackage {
        schedule_id: package.schedule_id,
        source_change_id: package
            .source_change_id
            .try_into()
            .map_err(|_| PackageError::InvalidRecord)?,
        instruction: SchedulePackageInstruction {
            instruction_id: package.instruction.instruction_id,
            text: package.instruction.text,
            source_revision_id: package
                .instruction
                .source_revision_id
                .try_into()
                .map_err(|_| PackageError::InvalidRecord)?,
        },
        definition: serde_json::from_value(
            serde_json::to_value(package.definition)
                .map_err(|_| PackageError::InvalidDefinition)?,
        )
        .map_err(|_| PackageError::InvalidDefinition)?,
        continuity: package
            .continuity
            .map(|summary| {
                Ok::<_, PackageError>(SchedulePackageContinuity {
                    text: summary.text,
                    source_run_id: summary
                        .source_run_id
                        .try_into()
                        .map_err(|_| PackageError::InvalidRecord)?,
                    source_target: summary.source_target,
                })
            })
            .transpose()?,
    })
}

fn encode_export(
    package: PortableSchedulePackage<SessionRef, EndpointRef>,
    budget: super::ResultByteBudget,
    context: ScheduleFailureContext,
) -> Result<collaboration_protocol::ScheduleExportResult, SchedulePackageFailure> {
    let failure = |error| {
        SchedulePackageFailure::Package(Box::new(schedule_failure(
            context.clone(),
            error,
            LocalMutationState::None,
            None,
        )))
    };
    let encoded = agent_automation::encode_schedule_package(&package, usize::MAX)
        .map_err(|error| failure(error.into()))?;
    // Reserve the actual escaped response and the larger round-trip import request.
    let import = serde_json::json!({
        "jsonrpc":"2.0", "id":"\u{0001}".repeat(128), "method":"schedule/import",
        "params":{"operationId":"00000000-0000-7000-8000-000000000000","packageUtf8":encoded,"overwrite":false}
    });
    let import_bytes = serde_json::to_vec(&import)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    let result = collaboration_protocol::ScheduleExportResult {
        package_utf8: encoded,
    };
    let response_bytes = budget
        .response_bytes(&result)
        .ok_or_else(|| failure(automation_storage::StorageError::InvalidRecord))?;
    let required_bytes = response_bytes.max(import_bytes);
    if required_bytes > budget.response_limit_bytes() {
        return Err(SchedulePackageFailure::Package(Box::new(
            ScheduleFailure::package_frame_limit(None, required_bytes),
        )));
    }
    Ok(result)
}
