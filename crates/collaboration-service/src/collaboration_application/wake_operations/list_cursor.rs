//! A Wake listing keeps local and central positions distinct inside a service-scoped cursor.
use automation_storage::WakeListPosition;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::UuidIdentity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ListPhase {
    Local { after: Option<WakeListPosition> },
    Central { after: Option<String> },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WakeListCursor {
    version: u8,
    service_id: UuidIdentity,
    position: ListPhase,
}
pub(super) fn encode(service_id: &UuidIdentity, position: ListPhase) -> Result<String, ()> {
    serde_json::to_vec(&WakeListCursor {
        version: 1,
        service_id: service_id.clone(),
        position,
    })
    .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
    .map_err(|_| ())
}
pub(super) fn decode(cursor: Option<&str>, service_id: &UuidIdentity) -> Result<ListPhase, ()> {
    let Some(cursor) = cursor else {
        return Ok(ListPhase::Local { after: None });
    };
    if cursor.is_empty() || cursor.len() > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
        return Err(());
    }
    let decoded: WakeListCursor =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?).map_err(|_| ())?;
    if decoded.version != 1 || decoded.service_id != *service_id {
        return Err(());
    }
    match &decoded.position {
        ListPhase::Local {
            after: Some(position),
        } if position.created_at_ms < 0
            || position.upper_created_at_ms < 0
            || (position.created_at_ms, &position.wakeup_id)
                >= (position.upper_created_at_ms, &position.upper_wakeup_id) =>
        {
            return Err(());
        }
        ListPhase::Central {
            after: Some(cursor),
        } if cursor.is_empty() => return Err(()),
        _ => {}
    }
    Ok(decoded.position)
}
