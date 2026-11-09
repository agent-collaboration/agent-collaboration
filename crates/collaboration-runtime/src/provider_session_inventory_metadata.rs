//! In-memory inventory labels and observed activity survive replay-log resets.
use serde::{Deserialize, Deserializer};
use session_event_model::{SessionItem, SessionItemKind};

#[derive(Default)]
pub(super) struct SessionInventoryMetadata {
    first_user_item: Option<SessionItem>,
    session_name: Option<String>,
    latest_activity_at_seconds: Option<i64>,
}

impl SessionInventoryMetadata {
    pub(super) fn observe_activity_at(&mut self, observed_at_seconds: i64) {
        self.latest_activity_at_seconds = Some(observed_at_seconds);
    }

    pub(super) fn latest_activity_at_seconds(&self) -> Option<i64> {
        self.latest_activity_at_seconds
    }

    pub(super) fn observe_item(&mut self, item: &SessionItem) {
        match item.kind {
            SessionItemKind::UserMessage => {
                if self
                    .first_user_item
                    .as_ref()
                    .is_none_or(|first| first.item_id == item.item_id)
                {
                    self.first_user_item = Some(item.clone());
                }
            }
            SessionItemKind::SessionInfo => {
                // SessionInfoUpdate is already serialized into this metadata item by
                // the ACP projection. Read only its title, including explicit clearing.
                if let Some(text) = &item.text {
                    match serde_json::from_str::<SessionTitleMetadata>(text) {
                        Ok(SessionTitleMetadata {
                            title: SessionTitleUpdate::Replace(title),
                        }) => {
                            self.session_name = title;
                        }
                        Ok(SessionTitleMetadata {
                            title: SessionTitleUpdate::Unchanged,
                        }) => {}
                        Err(_) => tracing::warn!("invalid provider session title metadata"),
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn preview(&self) -> &str {
        self.first_user_item
            .as_ref()
            .and_then(|item| item.text.as_deref())
            .unwrap_or_default()
    }

    pub(super) fn name(&self) -> Option<&str> {
        self.session_name.as_deref()
    }
}

#[derive(Deserialize)]
struct SessionTitleMetadata {
    #[serde(default)]
    title: SessionTitleUpdate,
}

#[derive(Default)]
enum SessionTitleUpdate {
    #[default]
    Unchanged,
    Replace(Option<String>),
}

impl<'de> Deserialize<'de> for SessionTitleUpdate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<String>::deserialize(deserializer).map(Self::Replace)
    }
}

#[cfg(test)]
#[path = "provider_session_inventory_metadata_test.rs"]
mod tests;
