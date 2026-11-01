//! Portable package text crosses the API boundary; filesystem paths remain client-local.
use crate::OperationId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleImportRequest {
    pub operation_id: OperationId,
    pub package_utf8: String,
    pub overwrite: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleExportResult {
    pub package_utf8: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulePackageImportRequest {
    pub operation_id: OperationId,
    pub package: SchedulePackage,
    pub overwrite: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulePackage {
    pub schedule_id: crate::ScheduleId,
    pub source_change_id: crate::NonEmptyText,
    pub instruction: SchedulePackageInstruction,
    #[schemars(schema_with = "package_definition_schema")]
    pub definition: crate::ScheduleDefinition,
    #[serde(default, deserialize_with = "Option::deserialize")]
    pub continuity: Option<SchedulePackageContinuity>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulePackageInstruction {
    pub instruction_id: crate::InstructionId,
    pub text: crate::InstructionText,
    pub source_revision_id: crate::NonEmptyText,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulePackageContinuity {
    pub text: crate::InstructionText,
    pub source_run_id: crate::NonEmptyText,
    pub source_target: crate::SessionRef,
}

fn package_definition_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "allOf":[crate::schedule_contract::central_schedule_schema(generator)],
        "properties":{"destination":{"properties":{"kind":{"enum":["unprepared","freshEachRunUnprepared"]}}}}
    })
}
