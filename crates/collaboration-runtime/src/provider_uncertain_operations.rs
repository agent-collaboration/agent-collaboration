//! Stored uncertainty with the facts needed for live admission and reopen.

use super::{
    ProviderOperationStore, ProviderOperationStoreError, decode_binding, decode_enum, encode_enum,
};
use collaboration_protocol::{
    ConversationBindingIdentity, OperationId, ProviderBindingIdentity, ProviderOperationEffect,
    ProviderOperationKind, ProviderOperationStage, SessionRef,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredUncertainOperation {
    pub operation: OperationId,
    pub kind: ProviderOperationKind,
    pub stage: ProviderOperationStage,
    pub target: Option<SessionRef>,
}

impl ProviderOperationStore {
    pub async fn stored_uncertain_operations(
        &mut self,
        binding: &ProviderBindingIdentity,
    ) -> Result<Vec<StoredUncertainOperation>, ProviderOperationStoreError> {
        let unknown = encode_enum(ProviderOperationEffect::Unknown)?;
        let candidates = sqlx::query!(
            "SELECT operation_id,operation_kind,stage,binding_json,target_service_id,target_endpoint_id,target_session_id
             FROM provider_operations WHERE effect=? ORDER BY operation_id",
            unknown,
        )
        .fetch_all(&mut self.connection)
        .await?;
        let mut rows = Vec::new();
        for candidate in candidates {
            let ConversationBindingIdentity::ExternalProvider { binding: stored } =
                decode_binding(&candidate.binding_json)?
            else {
                continue;
            };
            if stored.endpoint != binding.endpoint
                || stored.runtime.provider != binding.runtime.provider
                || stored.generation != binding.generation
            {
                continue;
            }
            let target = match (
                candidate.target_service_id,
                candidate.target_endpoint_id,
                candidate.target_session_id,
            ) {
                (None, None, None) => None,
                (Some(service_id), Some(endpoint_id), Some(session_id)) => Some(SessionRef {
                    endpoint: collaboration_protocol::EndpointRef {
                        service_id: service_id
                            .try_into()
                            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                        endpoint_id: endpoint_id
                            .try_into()
                            .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                    },
                    session_id: session_id
                        .try_into()
                        .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                }),
                _ => return Err(ProviderOperationStoreError::InvalidRecord),
            };
            rows.push(StoredUncertainOperation {
                operation: candidate
                    .operation_id
                    .try_into()
                    .map_err(|_| ProviderOperationStoreError::InvalidRecord)?,
                kind: decode_enum(candidate.operation_kind)?,
                stage: decode_enum(candidate.stage)?,
                target,
            });
        }
        Ok(rows)
    }
}
