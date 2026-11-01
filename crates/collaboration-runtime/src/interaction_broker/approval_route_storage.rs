//! Decode stored routes and validate their creator provenance before use.
use codex_acp_adapter::{ApprovalBrokerError, ApprovalRoute};
use collaboration_protocol::UuidIdentity;
use std::{collections::BTreeMap, path::Path};

pub(super) fn stored_route_is_eligible(route: &ApprovalRoute, service_id: &UuidIdentity) -> bool {
    route.approver.endpoint.service_id == *service_id
        && match &route.source_installation {
            None => route.created_by.endpoint.service_id == *service_id,
            Some(source) => {
                source.as_str() == String::from(route.created_by.endpoint.service_id.clone())
            }
        }
}

pub(super) async fn read_routes(
    path: &Path,
    service_id: &UuidIdentity,
) -> Result<BTreeMap<String, ApprovalRoute>, ApprovalBrokerError> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(_) => return Err(ApprovalBrokerError::Unavailable),
    };
    let stored = serde_json::from_slice::<Vec<ApprovalRoute>>(&bytes)
        .map_err(|_| ApprovalBrokerError::Unavailable)?;
    if !stored
        .iter()
        .all(|route| stored_route_is_eligible(route, service_id))
    {
        return Err(ApprovalBrokerError::Unavailable);
    }
    Ok(stored
        .into_iter()
        .map(|route| (route.thread_id.clone(), route))
        .collect())
}
