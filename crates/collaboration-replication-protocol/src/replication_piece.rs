use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    CommitId, DigestError, Epoch, PieceDigest, ProjectId, ReplicationPosition, TransactionPart,
};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum SqlValueWire {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SqlValueError {
    #[error("blob value is not valid base64")]
    InvalidBase64,
}

impl Serialize for SqlValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            Self::Null => SqlValueWire::Null,
            Self::Integer(value) => SqlValueWire::Integer(*value),
            Self::Real(value) => {
                if !value.is_finite() {
                    return Err(serde::ser::Error::custom("SQL real must be finite"));
                }
                SqlValueWire::Real(*value)
            }
            Self::Text(value) => SqlValueWire::Text(value.clone()),
            Self::Blob(value) => SqlValueWire::Blob(encode_base64(value)),
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SqlValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match SqlValueWire::deserialize(deserializer)? {
            SqlValueWire::Null => Ok(Self::Null),
            SqlValueWire::Integer(value) => Ok(Self::Integer(value)),
            SqlValueWire::Real(value) => Ok(Self::Real(value)),
            SqlValueWire::Text(value) => Ok(Self::Text(value)),
            SqlValueWire::Blob(value) => decode_base64(&value)
                .map(Self::Blob)
                .map_err(D::Error::custom),
        }
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut values = chunk.iter().copied();
        let first = values.next().unwrap_or(0);
        let second = values.next();
        let third = values.next();
        push_base64_character(&mut encoded, first >> 2);
        push_base64_character(
            &mut encoded,
            ((first & 0b11) << 4) | second.map_or(0, |value| value >> 4),
        );
        match second {
            Some(value) => push_base64_character(
                &mut encoded,
                ((value & 0b1111) << 2) | third.map_or(0, |next| next >> 6),
            ),
            None => encoded.push('='),
        }
        match third {
            Some(value) => push_base64_character(&mut encoded, value & 0b111111),
            None => encoded.push('='),
        }
    }
    encoded
}

fn push_base64_character(output: &mut String, value: u8) {
    if let Some(character) = BASE64_ALPHABET.get(usize::from(value)) {
        output.push(char::from(*character));
    }
}

fn decode_base64(value: &str) -> Result<Vec<u8>, SqlValueError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.len().is_multiple_of(4) {
        return Err(SqlValueError::InvalidBase64);
    }

    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut values = chunk.iter().copied();
        let first = values
            .next()
            .and_then(base64_value)
            .ok_or(SqlValueError::InvalidBase64)?;
        let second_byte = values.next().ok_or(SqlValueError::InvalidBase64)?;
        let second = base64_value(second_byte).ok_or(SqlValueError::InvalidBase64)?;
        let third_byte = values.next().ok_or(SqlValueError::InvalidBase64)?;
        let fourth_byte = values.next().ok_or(SqlValueError::InvalidBase64)?;
        let third = if third_byte == b'=' {
            if fourth_byte != b'=' {
                return Err(SqlValueError::InvalidBase64);
            }
            None
        } else {
            Some(base64_value(third_byte).ok_or(SqlValueError::InvalidBase64)?)
        };
        let fourth = if fourth_byte == b'=' {
            None
        } else {
            Some(base64_value(fourth_byte).ok_or(SqlValueError::InvalidBase64)?)
        };

        decoded.push((first << 2) | (second >> 4));
        if let Some(third) = third {
            decoded.push((second << 4) | (third >> 2));
            if let Some(fourth) = fourth {
                decoded.push((third << 6) | fourth);
            }
        } else if fourth.is_some() || second_byte == b'=' {
            return Err(SqlValueError::InvalidBase64);
        }
    }
    Ok(decoded)
}

fn base64_value(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowChange {
    Upsert {
        table: String,
        key: Vec<SqlValue>,
        values: Vec<SqlValue>,
    },
    Delete {
        table: String,
        key: Vec<SqlValue>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("table name must not be empty")]
pub struct RowChangeError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum RowChangeWire {
    Upsert {
        table: String,
        key: Vec<SqlValue>,
        values: Vec<SqlValue>,
    },
    Delete {
        table: String,
        key: Vec<SqlValue>,
    },
}

impl RowChange {
    pub fn upsert(
        table: impl Into<String>,
        key: Vec<SqlValue>,
        values: Vec<SqlValue>,
    ) -> Result<Self, RowChangeError> {
        let table = table.into();
        if table.is_empty() {
            return Err(RowChangeError);
        }
        Ok(Self::Upsert { table, key, values })
    }

    pub fn delete(table: impl Into<String>, key: Vec<SqlValue>) -> Result<Self, RowChangeError> {
        let table = table.into();
        if table.is_empty() {
            return Err(RowChangeError);
        }
        Ok(Self::Delete { table, key })
    }
}

impl Serialize for RowChange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            Self::Upsert { table, key, values } => RowChangeWire::Upsert {
                table: table.clone(),
                key: key.clone(),
                values: values.clone(),
            },
            Self::Delete { table, key } => RowChangeWire::Delete {
                table: table.clone(),
                key: key.clone(),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RowChange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match RowChangeWire::deserialize(deserializer)? {
            RowChangeWire::Upsert { table, key, values } => {
                Self::upsert(table, key, values).map_err(D::Error::custom)
            }
            RowChangeWire::Delete { table, key } => {
                Self::delete(table, key).map_err(D::Error::custom)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReplicatedTransaction {
    Rows {
        commit_id: CommitId,
        rows: Vec<RowChange>,
    },
    SchemaStep {
        commit_id: CommitId,
        migration_version: u64,
        statements: Vec<String>,
    },
}

impl ReplicatedTransaction {
    #[must_use]
    pub const fn commit_id(&self) -> CommitId {
        match self {
            Self::Rows { commit_id, .. } | Self::SchemaStep { commit_id, .. } => *commit_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSummary {
    pub status: String,
    pub latest_record_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PieceContents {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub continues_from: ReplicationPosition,
    pub through: ReplicationPosition,
    pub transactions: Vec<ReplicatedTransaction>,
    pub part: Option<TransactionPart>,
    pub summary: Option<ProjectSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub continues_from: ReplicationPosition,
    pub through: ReplicationPosition,
    pub transactions: Vec<ReplicatedTransaction>,
    pub part: Option<TransactionPart>,
    pub summary: Option<ProjectSummary>,
    pub digest: PieceDigest,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PieceError {
    #[error("piece through position must be after continuesFrom and in its writer epoch")]
    InvalidPosition,
    #[error("part must name this epoch and through commit, with exactly one transaction fragment")]
    InvalidPart,
    #[error("transactions must be ordered and end at through")]
    InvalidTransactions,
    #[error("only a complete piece or final transaction part carries a nonempty summary")]
    InvalidSummary,
    #[error(transparent)]
    Digest(#[from] DigestError),
}

impl PieceContents {
    pub fn validate(&self) -> Result<(), PieceError> {
        if self.through <= self.continues_from
            || self.through.epoch != self.epoch
            || self.continues_from.epoch > self.epoch
        {
            return Err(PieceError::InvalidPosition);
        }
        if let Some(part) = &self.part
            && (part.count == 0
                || part.index >= part.count
                || part.transaction.epoch != self.epoch
                || part.transaction.commit_id != self.through.commit_id
                || self.transactions.len() != 1)
        {
            return Err(PieceError::InvalidPart);
        }
        let requires_summary = self
            .part
            .as_ref()
            .is_none_or(|part| part.index == part.count - 1);
        if requires_summary != self.summary.is_some()
            || self
                .summary
                .as_ref()
                .is_some_and(|summary| summary.status.is_empty())
        {
            return Err(PieceError::InvalidSummary);
        }
        let mut previous = if self.continues_from.epoch == self.epoch {
            self.continues_from.commit_id
        } else {
            CommitId::new(0)
        };
        for transaction in &self.transactions {
            if transaction.commit_id() <= previous {
                return Err(PieceError::InvalidTransactions);
            }
            previous = transaction.commit_id();
        }
        if self
            .transactions
            .last()
            .is_some_and(|transaction| transaction.commit_id() != self.through.commit_id)
        {
            return Err(PieceError::InvalidTransactions);
        }
        Ok(())
    }
}

impl Piece {
    pub fn new(contents: PieceContents) -> Result<Self, PieceError> {
        contents.validate()?;
        let digest = piece_content_digest(&contents)?;
        Ok(Self::with_digest(contents, digest))
    }

    fn with_digest(contents: PieceContents, digest: PieceDigest) -> Self {
        Self {
            project: contents.project,
            epoch: contents.epoch,
            continues_from: contents.continues_from,
            through: contents.through,
            transactions: contents.transactions,
            part: contents.part,
            summary: contents.summary,
            digest,
        }
    }

    pub fn recomputed_digest(&self) -> Result<PieceDigest, DigestError> {
        PieceDigest::of_canonical(&(
            &self.project,
            self.epoch,
            self.continues_from,
            self.through,
            &self.transactions,
            &self.part,
            &self.summary,
        ))
    }
}

fn piece_content_digest(contents: &PieceContents) -> Result<PieceDigest, DigestError> {
    PieceDigest::of_canonical(&(
        &contents.project,
        contents.epoch,
        contents.continues_from,
        contents.through,
        &contents.transactions,
        &contents.part,
        &contents.summary,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPiece {
    project: ProjectId,
    epoch: Epoch,
    continues_from: ReplicationPosition,
    through: ReplicationPosition,
    transactions: Vec<ReplicatedTransaction>,
    part: Option<TransactionPart>,
    summary: Option<ProjectSummary>,
    digest: PieceDigest,
}

impl<'de> Deserialize<'de> for Piece {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPiece::deserialize(deserializer)?;
        let contents = PieceContents {
            project: raw.project,
            epoch: raw.epoch,
            continues_from: raw.continues_from,
            through: raw.through,
            transactions: raw.transactions,
            part: raw.part,
            summary: raw.summary,
        };
        contents.validate().map_err(D::Error::custom)?;
        // The copy decides pieceConflict for a digest mismatch; decoding preserves
        // the sender's digest so the request still receives its typed answer.
        Ok(Self::with_digest(contents, raw.digest))
    }
}
