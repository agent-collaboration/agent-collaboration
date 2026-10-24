//! Hash the typed semantic request, excluding its request identity.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;

use crate::{
    ActingForIdentity, BoardOperation, CanonicalHash, Identity, ProjectCreateRequest, ProjectId,
    ProjectRequest, ProtocolVersion,
};

#[derive(Debug, thiserror::Error)]
pub enum CanonicalRequestError {
    #[error("request cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("canonical digest cannot be represented: {0}")]
    Digest(#[from] crate::TextValidationError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalProjectRequest<'request> {
    project_id: &'request ProjectId,
    actor: &'request Identity,
    acting_for: &'request Option<ActingForIdentity>,
    protocol_version: ProtocolVersion,
    operation: &'request BoardOperation,
}

pub fn canonical_hash(request: &ProjectRequest) -> Result<CanonicalHash, CanonicalRequestError> {
    let operation = canonical_operation(&request.operation);
    let semantic = CanonicalProjectRequest {
        project_id: &request.project_id,
        actor: &request.actor,
        acting_for: &request.acting_for,
        protocol_version: request.protocol_version,
        operation: &operation,
    };
    let encoded = serde_json::to_vec(&semantic)?;
    let digest = Sha256::digest(encoded);
    Ok(CanonicalHash::try_from(format!("{digest:x}"))?)
}

/// The directory hash omits the initial description; creating-input identity includes it.
pub fn creation_hash(
    request: &ProjectCreateRequest,
) -> Result<CanonicalHash, CanonicalRequestError> {
    let bytes = serde_json::to_vec(&(
        &request.project_id,
        &request.board_id,
        &request.title,
        request
            .description
            .as_ref()
            .map_or("", crate::Description::as_str),
        &request.actor,
        &request.acting_for,
        request.protocol_version,
    ))?;
    Ok(CanonicalHash::try_from(format!(
        "{:x}",
        Sha256::digest(bytes)
    ))?)
}

fn canonical_operation(operation: &BoardOperation) -> Cow<'_, BoardOperation> {
    if let BoardOperation::TaskCreate { .. } = operation {
        let mut canonical = operation.clone();
        if let BoardOperation::TaskCreate {
            blocked_by,
            related_to,
            ..
        } = &mut canonical
        {
            // These become unique link rows, not ordered arrays on the task (TK-2).
            blocked_by.sort();
            blocked_by.dedup();
            related_to.sort();
            related_to.dedup();
        }
        Cow::Owned(canonical)
    } else {
        Cow::Borrowed(operation)
    }
}
