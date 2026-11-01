//! Recipient-authorized access to every item covered by a notification listing.
use crate::{PushId, PushRecordNotice, SessionRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationMembersRequest {
    pub caller: SessionRef,
    pub listing_id: PushId,
    pub after: Option<PushId>,
    #[schemars(schema_with = "notification_page_limit_schema")]
    pub limit: u32,
}

fn notification_page_limit_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let limit = crate::limits::LimitName::NotificationListingMaxEntries.definition();
    schemars::json_schema!({"type":"integer","minimum":limit.minimum,"maximum":limit.maximum,"multipleOf":limit.step,
        "description":format!("{}: {}..={} {}. {}",limit.name,limit.minimum,limit.maximum,limit.unit,limit.description)})
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationMembersPage {
    pub listing_id: PushId,
    pub records: Vec<PushRecordNotice>,
    pub next: Option<PushId>,
}
