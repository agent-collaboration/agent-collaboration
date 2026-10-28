use message_board::ProjectId;
use message_board_storage::{ImportRowKeys, ImportRowKind};
use sha2::{Digest, Sha256};

/// Derivation policy belongs to the importer; storage requests its helper keys.
pub struct DerivedImportKeys {
    prefix: [u8; 6],
}

impl DerivedImportKeys {
    pub fn new(project: &ProjectId) -> Self {
        let mut prefix = [0; 6];
        // ProjectId has already checked the canonical UUIDv7 representation.
        let compact = project
            .as_str()
            .bytes()
            .filter(|byte| *byte != b'-')
            .take(12)
            .collect::<Vec<_>>();
        for (slot, [high, low]) in prefix.iter_mut().zip(compact.as_chunks::<2>().0) {
            let digit = |value: u8| {
                if value <= b'9' {
                    value - b'0'
                } else {
                    value - b'a' + 10
                }
            };
            *slot = digit(*high) * 16 + digit(*low);
        }
        Self { prefix }
    }

    pub fn derive(&self, kind: &str, stable_parts: &[&str]) -> Result<String, serde_json::Error> {
        let content = serde_json::to_string(&("board-import-v1", kind, stable_parts))?;
        let digest = Sha256::digest(content.as_bytes());
        let mut bytes = [0; 16];
        for (target, source) in bytes.iter_mut().take(6).zip(self.prefix) {
            *target = source;
        }
        for (target, source) in bytes.iter_mut().skip(6).zip(digest.iter()) {
            *target = *source;
        }
        if let Some(version) = bytes.get_mut(6) {
            *version = (*version & 15) | 0x70;
        }
        if let Some(variant) = bytes.get_mut(8) {
            *variant = (*variant & 63) | 0x80;
        }
        Ok(uuid::Uuid::from_bytes(bytes).to_string())
    }
}

impl ImportRowKeys for DerivedImportKeys {
    fn row_key(&self, kind: ImportRowKind, stable_parts: &[&str]) -> String {
        let name = match kind {
            ImportRowKind::Actor => "actor",
            ImportRowKind::EpochLine => "epochLine",
            ImportRowKind::TaskLink => "taskLink",
            ImportRowKind::PostReference => "postReference",
            ImportRowKind::ThreadParticipation => "threadParticipation",
            ImportRowKind::WorkTrailTag => "workTrailTag",
            ImportRowKind::WorkTrailLink => "workTrailLink",
            ImportRowKind::ReadMark => "readMark",
            ImportRowKind::ThreadWatch => "threadWatch",
            ImportRowKind::ProjectWatch => "projectWatch",
            ImportRowKind::SubscriptionMembership => "subscriptionMembership",
            ImportRowKind::FeedEntry => "feedEntry",
            ImportRowKind::HandoffFrontier => "handoffFrontier",
        };
        // The storage trait returns text, so an encoding failure deliberately
        // returns an invalid key which storage rejects as InvalidImportRowKey.
        self.derive(name, stable_parts).unwrap_or_default()
    }
}
