//! Source-scoped preview loading and publication for one picker view.
use std::collections::BTreeMap;

use agent_session_inventory::sessions::SessionConversationPreview;
use agent_session_inventory::sessions::SessionConversationSource;
use agent_session_inventory::sessions::SessionPickerIdentity;
use agent_session_inventory::sessions::SessionPickerRecord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationPreviewKey {
    pub(crate) identity: SessionPickerIdentity,
    pub(crate) source: SessionConversationSource,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversationPreviewLoadRequest {
    pub(crate) key: ConversationPreviewKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConversationPreviewLoadState {
    Loading,
    Loaded(SessionConversationPreview),
}

struct ConversationPreviewEntry {
    key: ConversationPreviewKey,
    state: ConversationPreviewLoadState,
}

#[derive(Default)]
pub(crate) struct ConversationPreviewCache {
    generation: u64,
    // A source change replaces the identity's slot; key equality also checks its home and view.
    entries: BTreeMap<SessionPickerIdentity, ConversationPreviewEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectedConversationPreview {
    pub(crate) preview: SessionConversationPreview,
    pub(crate) load_request: Option<ConversationPreviewLoadRequest>,
}

impl ConversationPreviewCache {
    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.entries.clear();
    }

    pub(crate) fn select_record(
        &self,
        record: &SessionPickerRecord,
    ) -> SelectedConversationPreview {
        let Some(source) = record.conversation_source.as_ref() else {
            return SelectedConversationPreview {
                preview: record.conversation.clone(),
                load_request: None,
            };
        };
        let key = ConversationPreviewKey {
            identity: record.identity.clone(),
            source: source.clone(),
            generation: self.generation,
        };
        let entry = self
            .entries
            .get(&key.identity)
            .filter(|entry| entry.key == key);
        match entry.map(|entry| &entry.state) {
            Some(ConversationPreviewLoadState::Loaded(preview)) => SelectedConversationPreview {
                preview: preview.clone(),
                load_request: None,
            },
            Some(ConversationPreviewLoadState::Loading) => SelectedConversationPreview {
                preview: record.conversation.clone(),
                load_request: None,
            },
            None => SelectedConversationPreview {
                preview: record.conversation.clone(),
                load_request: Some(ConversationPreviewLoadRequest { key }),
            },
        }
    }

    pub(crate) fn start_loading(&mut self, request: &ConversationPreviewLoadRequest) {
        if request.key.generation != self.generation {
            return;
        }
        self.entries.insert(
            request.key.identity.clone(),
            ConversationPreviewEntry {
                key: request.key.clone(),
                state: ConversationPreviewLoadState::Loading,
            },
        );
    }

    pub(crate) fn complete_load(
        &mut self,
        request: &ConversationPreviewLoadRequest,
        preview: SessionConversationPreview,
    ) {
        if request.key.generation != self.generation {
            return;
        }
        if let Some(entry) = self.entries.get_mut(&request.key.identity)
            && entry.key == request.key
            && matches!(entry.state, ConversationPreviewLoadState::Loading)
        {
            entry.state = ConversationPreviewLoadState::Loaded(preview);
        }
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
