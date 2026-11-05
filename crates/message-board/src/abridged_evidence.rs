//! Bounded refusal evidence commits to the complete ordered rule result.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::{Assignment, CanonicalHash, TaskId};

pub const MAX_EVIDENCE_ENTRIES: usize = 256;

mod sealed {
    pub trait EvidenceEntry {}
    impl EvidenceEntry for crate::TaskId {}
    impl EvidenceEntry for crate::Assignment {}
    impl EvidenceEntry for (crate::RecordRef, crate::RecordPlacement) {}
}

/// The closed evidence vocabulary has an infallible canonical JSON encoding.
/// Object fields are emitted in lexical order; IDs are canonical ASCII UUIDs.
pub trait EvidenceEntry: sealed::EvidenceEntry {
    #[doc(hidden)]
    fn canonical_evidence_json(&self) -> serde_json::Value;
}

impl EvidenceEntry for TaskId {
    fn canonical_evidence_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.as_str().to_owned())
    }
}

impl EvidenceEntry for Assignment {
    fn canonical_evidence_json(&self) -> serde_json::Value {
        let (kind, subject) = match self {
            Self::Lead(id) => ("lead", id.as_str()),
            Self::Advisor(id) => ("advisor", id.as_str()),
            Self::Implementer(id) => ("implementer", id.as_str()),
            Self::Verifier(id) => ("verifier", id.as_str()),
            Self::Reviewer(id) => ("reviewer", id.as_str()),
            Self::Operator(id) => ("operator", id.as_str()),
        };
        let mut object = serde_json::Map::new();
        object.insert(
            "kind".to_owned(),
            serde_json::Value::String(kind.to_owned()),
        );
        object.insert(
            "subject".to_owned(),
            serde_json::Value::String(subject.to_owned()),
        );
        serde_json::Value::Object(object)
    }
}

impl EvidenceEntry for (crate::RecordRef, crate::RecordPlacement) {
    fn canonical_evidence_json(&self) -> serde_json::Value {
        // This closed pair contains only validated IDs, integer positions and origins.
        canonical_object_order(serde_json::json!(self))
    }
}

fn canonical_object_order(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_object_order(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(array) => {
            serde_json::Value::Array(array.into_iter().map(canonical_object_order).collect())
        }
        scalar => scalar,
    }
}

pub(crate) fn evidence_digest<T: EvidenceEntry>(entries: &[T]) -> CanonicalHash {
    let mut digest = Sha256::new();
    digest.update(b"[");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            digest.update(b",");
        }
        digest.update(entry.canonical_evidence_json().to_string().as_bytes());
    }
    digest.update(b"]");
    CanonicalHash::from_sha256(digest.finalize().into())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbridgedEvidence<T> {
    #[schemars(length(max = 256))]
    entries: Vec<T>,
    total_count: u64,
    digest: CanonicalHash,
    abridged: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidenceWire<T> {
    entries: Vec<T>,
    total_count: u64,
    digest: CanonicalHash,
    abridged: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EvidenceValidationError {
    #[error("evidence count and prefix must agree")]
    Count,
    #[error("complete evidence digest must match")]
    Digest,
}

impl<T: EvidenceEntry> AbridgedEvidence<T> {
    /// Reconstruct a validated prefix while retaining its original full commitment.
    pub fn from_parts(
        entries: Vec<T>,
        total_count: u64,
        digest: CanonicalHash,
        abridged: bool,
    ) -> Result<Self, EvidenceValidationError> {
        if entries.len() > MAX_EVIDENCE_ENTRIES
            || u64::try_from(entries.len()).ok() != Some(total_count.min(256))
            || abridged != (total_count > 256)
        {
            return Err(EvidenceValidationError::Count);
        }
        // The omitted entries are unavailable for an abridged commitment.
        if !abridged && evidence_digest(&entries) != digest {
            return Err(EvidenceValidationError::Digest);
        }
        Ok(Self {
            entries,
            total_count,
            digest,
            abridged,
        })
    }

    #[must_use]
    pub fn from_full(mut entries: Vec<T>) -> Self {
        const {
            assert!(usize::BITS <= u64::BITS);
        }
        // The compile-time width check makes this conversion lossless on every target.
        let total_count = entries.len() as u64;
        let digest = evidence_digest(&entries);
        entries.truncate(MAX_EVIDENCE_ENTRIES);
        Self {
            entries,
            total_count,
            digest,
            abridged: total_count > 256,
        }
    }

    #[must_use]
    pub fn entries(&self) -> &[T] {
        &self.entries
    }
    #[must_use]
    pub fn total_count(&self) -> u64 {
        self.total_count
    }
    #[must_use]
    pub fn digest(&self) -> &CanonicalHash {
        &self.digest
    }
    #[must_use]
    pub fn is_abridged(&self) -> bool {
        self.abridged
    }
}

impl<'de, T: Deserialize<'de> + EvidenceEntry> Deserialize<'de> for AbridgedEvidence<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = EvidenceWire::<T>::deserialize(deserializer)?;
        Self::from_parts(wire.entries, wire.total_count, wire.digest, wire.abridged)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "abridged_evidence_test.rs"]
mod tests;
