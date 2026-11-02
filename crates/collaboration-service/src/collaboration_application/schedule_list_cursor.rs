//! Local Schedule positions and opaque central tokens have separate service-scoped phases.
use automation_storage::{AutomationCollection, AutomationListPosition};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::{ScheduleId, UuidIdentity};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ScheduleListPhase {
    Local {
        after: Option<AutomationListPosition>,
    },
    Central {
        after: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleListCursor {
    version: u8,
    service_id: UuidIdentity,
    collection: Collection,
    position: ScheduleListPhase,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Collection {
    Schedule,
}
pub(super) fn encode(service: &UuidIdentity, position: ScheduleListPhase) -> Result<String, ()> {
    serde_json::to_vec(&ScheduleListCursor {
        version: 1,
        service_id: service.clone(),
        collection: Collection::Schedule,
        position,
    })
    .map(|value| URL_SAFE_NO_PAD.encode(value))
    .map_err(|_| ())
}
pub(super) fn decode(value: Option<&str>, service: &UuidIdentity) -> Result<ScheduleListPhase, ()> {
    let Some(value) = value else {
        return Ok(ScheduleListPhase::Local { after: None });
    };
    if value.is_empty() || value.len() > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
        return Err(());
    }
    let cursor: ScheduleListCursor =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?).map_err(|_| ())?;
    if cursor.version != 1 || cursor.service_id != *service {
        return Err(());
    }
    match &cursor.position {
        ScheduleListPhase::Local {
            after: Some(position),
        } => {
            position
                .validate_for(&AutomationCollection::Schedules)
                .map_err(|_| ())?;
            for key in [&position.upper, &position.last] {
                ScheduleId::try_from(key.resource_id.clone()).map_err(|_| ())?;
            }
        }
        ScheduleListPhase::Central { after: Some(value) } if value.is_empty() => return Err(()),
        _ => {}
    }
    Ok(cursor.position)
}
