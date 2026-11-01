//! Consumer-owned grouping; input rows have already been filtered and sorted.
use agent_session_inventory::session_provider_family::SessionProviderFamily;
use agent_session_inventory::{
    sessions::{SessionPickerIdentity, SessionPickerRecord},
    source_inventory_request::PickerSourceContext,
};
use chrono::{DateTime, Datelike, Days, TimeZone, Utc};
use collaboration_protocol::PushKind;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionGroupFacet {
    #[default]
    None,
    StartedBy,
    Day,
    Folder,
    Provider,
    Machine,
}

impl SessionGroupFacet {
    pub const CHOICES: [Self; 6] = [
        Self::None,
        Self::StartedBy,
        Self::Day,
        Self::Folder,
        Self::Provider,
        Self::Machine,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::StartedBy => "Started by",
            Self::Day => "Day",
            Self::Folder => "Folder",
            Self::Provider => "Provider",
            Self::Machine => "Machine",
        }
    }

    pub const fn stored_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::StartedBy => "started-by",
            Self::Day => "day",
            Self::Folder => "folder",
            Self::Provider => "provider",
            Self::Machine => "machine",
        }
    }

    pub fn from_stored_value(value: &str) -> Self {
        Self::CHOICES
            .into_iter()
            .find(|facet| facet.stored_value() == value)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionGroupHeading {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionGroup {
    pub heading: SessionGroupHeading,
    pub keys: Vec<SessionPickerIdentity>,
}

pub fn compute_session_groups<'a, TZone: TimeZone>(
    facet: SessionGroupFacet,
    rows: impl IntoIterator<Item = &'a SessionPickerRecord>,
    now: DateTime<Utc>,
    time_zone: &TZone,
    machine_order: &[PickerSourceContext],
) -> Vec<SessionGroup> {
    if facet == SessionGroupFacet::None {
        return Vec::new();
    }
    let mut groups: BTreeMap<(usize, String), SessionGroup> = BTreeMap::new();
    let mut machine_sources = machine_order.to_vec();
    for row in rows {
        let (order, name) = match facet {
            SessionGroupFacet::None => return Vec::new(),
            SessionGroupFacet::StartedBy => started_by_group(&row.first_user_message),
            SessionGroupFacet::Day => day_group(row.recency_at_ms, now, time_zone),
            SessionGroupFacet::Folder => {
                let folder = row
                    .normalized_cwd
                    .as_deref()
                    .or(row.cwd.as_deref())
                    .filter(|folder| !folder.is_empty());
                match folder {
                    Some(folder) => (0, folder.to_owned()),
                    None => (1, "Unknown folder".to_owned()),
                }
            }
            SessionGroupFacet::Provider => {
                use collaboration_protocol::ProviderKind;
                let order = match row.provider_family {
                    SessionProviderFamily::Codex => 0,
                    SessionProviderFamily::External(ProviderKind::ClaudeCode) => 1,
                    SessionProviderFamily::External(ProviderKind::Cursor) => 2,
                    SessionProviderFamily::Unclassified => 3,
                };
                (order, row.provider_family.label().to_owned())
            }
            SessionGroupFacet::Machine => {
                let source = row
                    .source_context
                    .clone()
                    .unwrap_or(PickerSourceContext::DefaultHosted);
                let order = if let Some(index) =
                    machine_sources.iter().position(|known| *known == source)
                {
                    index
                } else {
                    machine_sources.push(source);
                    machine_sources.len() - 1
                };
                (order, row.machine_label().to_owned())
            }
        };
        let sort_name = if facet == SessionGroupFacet::Machine {
            String::new()
        } else {
            name.clone()
        };
        let group = groups
            .entry((order, sort_name))
            .or_insert_with(|| SessionGroup {
                heading: SessionGroupHeading { name, count: 0 },
                keys: Vec::new(),
            });
        group.keys.push(row.identity.clone());
        group.heading.count = group.keys.len();
    }
    groups.into_values().collect()
}

fn started_by_group(first_message: &str) -> (usize, String) {
    let (order, name) =
        match collaboration_protocol::parse_push_line(first_message).map(|line| line.kind) {
            None => (0, "You"),
            Some(PushKind::DirectMessage) => (1, "Messages"),
            Some(PushKind::BoardNotification) => (2, "Board"),
            Some(PushKind::ScheduleRun | PushKind::Wake) => (3, "Schedules and wakes"),
            Some(PushKind::Question | PushKind::Approval) => (4, "Questions and approvals"),
            Some(PushKind::SubscriptionActivity | PushKind::SubscriptionExpiry) => {
                (5, "Thread activity")
            }
        };
    (order, name.to_owned())
}

fn day_group<TZone: TimeZone>(
    recency_at_ms: Option<i64>,
    now: DateTime<Utc>,
    time_zone: &TZone,
) -> (usize, String) {
    let today = now.with_timezone(time_zone).date_naive();
    let yesterday = today.pred_opt();
    let monday =
        today.checked_sub_days(Days::new(u64::from(today.weekday().num_days_from_monday())));
    let recency = recency_at_ms
        .and_then(DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.with_timezone(time_zone).date_naive());
    let (order, name) = match recency {
        Some(date) if date >= today => (0, "Today"),
        Some(date) if Some(date) == yesterday => (1, "Yesterday"),
        Some(date) if monday.is_some_and(|monday| date >= monday) => (2, "This week"),
        Some(_) | None => (3, "Older"),
    };
    (order, name.to_owned())
}

#[cfg(test)]
#[path = "session_grouping_tests.rs"]
mod tests;
