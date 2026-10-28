//! The digest covers every delivery input; stored and wire decoding rechecks it.
use crate::{MailBody, MailDigest, MailItemId, MailKind, MailSender, MailValidationError};
use chrono::{DateTime, Utc};
use message_board::SessionRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "MailItemFields"
)]
pub struct MailItem {
    id: MailItemId,
    sender: MailSender,
    recipient: SessionRef,
    valid_until: Option<DateTime<Utc>>,
    body: MailBody,
    digest: MailDigest,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailItemContent {
    pub id: MailItemId,
    pub sender: MailSender,
    pub recipient: SessionRef,
    pub valid_until: Option<DateTime<Utc>>,
    pub body: MailBody,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailItemFields {
    id: MailItemId,
    sender: MailSender,
    recipient: SessionRef,
    valid_until: Option<DateTime<Utc>>,
    body: MailBody,
    digest: MailDigest,
}
impl TryFrom<MailItemFields> for MailItem {
    type Error = MailValidationError;
    fn try_from(fields: MailItemFields) -> Result<Self, Self::Error> {
        let item = Self::new(MailItemContent {
            id: fields.id,
            sender: fields.sender,
            recipient: fields.recipient,
            valid_until: fields.valid_until,
            body: fields.body,
        })?;
        if fields.digest != item.digest {
            return Err(MailValidationError { field: "digest" });
        }
        Ok(item)
    }
}
impl MailItem {
    pub fn new(content: MailItemContent) -> Result<Self, MailValidationError> {
        content.body.validate_sender(&content.sender)?;
        let digest = digest_content(&content)?;
        Ok(Self {
            id: content.id,
            sender: content.sender,
            recipient: content.recipient,
            valid_until: content.valid_until,
            body: content.body,
            digest,
        })
    }
    #[must_use]
    pub fn id(&self) -> &MailItemId {
        &self.id
    }
    #[must_use]
    pub fn sender(&self) -> &MailSender {
        &self.sender
    }
    #[must_use]
    pub fn recipient(&self) -> &SessionRef {
        &self.recipient
    }
    #[must_use]
    pub fn valid_until(&self) -> Option<DateTime<Utc>> {
        self.valid_until
    }
    #[must_use]
    pub fn body(&self) -> &MailBody {
        &self.body
    }
    #[must_use]
    pub fn digest(&self) -> &MailDigest {
        &self.digest
    }
    #[must_use]
    pub const fn kind(&self) -> MailKind {
        self.body.kind()
    }
}
fn digest_content(content: &MailItemContent) -> Result<MailDigest, MailValidationError> {
    // E30 has integer fields beyond the IEEE-754 exact range. Sort typed JSON
    // object keys without rounding those values through floating point.
    let mut value = serde_json::to_value(content).map_err(|_| MailValidationError {
        field: "canonicalContent",
    })?;
    value.sort_all_objects();
    let canonical = serde_json::to_vec(&value).map_err(|_| MailValidationError {
        field: "canonicalContent",
    })?;
    format!("{:x}", Sha256::digest(canonical)).try_into()
}
