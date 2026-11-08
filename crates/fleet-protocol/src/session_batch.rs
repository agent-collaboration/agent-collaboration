use crate::{FrameId, unit_budget::encoded_json_size};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

pub const MAX_BATCH_ITEMS: usize = 32;
pub const MAX_BATCH_BYTES: usize = 1024 * 1024;

/// Compare typed operation variants without inspecting serialized JSON.
pub trait BatchRequest {
    fn same_request_kind(&self, other: &Self) -> bool;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchItem<Body> {
    pub send_id: FrameId,
    pub body: Body,
}

#[derive(Debug, thiserror::Error)]
pub enum BatchValidationError {
    #[error("a batch must contain at least one item")]
    Empty,
    #[error("a batch exceeds 32 items")]
    TooManyItems,
    #[error("a batch exceeds 1 MiB")]
    TooManyBytes,
    #[error("a request batch mixes operation types")]
    MixedRequestKinds,
    #[error("cannot encode a batch")]
    Encoding(#[from] serde_json::Error),
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct RequestBatch<Body>(Vec<BatchItem<Body>>);

#[derive(Serialize)]
#[serde(transparent)]
pub struct AnswerBatch<Body>(Vec<BatchItem<Body>>);

// Measure the actual frame envelope as well as its items, without allocating
// another encoded payload. This shape is the session frame's existing contract.
#[derive(Serialize)]
struct BatchEnvelope<'a, Body> {
    kind: &'static str,
    value: &'a [BatchItem<Body>],
}

fn validate_size<Body: Serialize>(
    kind: &'static str,
    items: &[BatchItem<Body>],
) -> Result<(), BatchValidationError> {
    if items.is_empty() {
        return Err(BatchValidationError::Empty);
    }
    if items.len() > MAX_BATCH_ITEMS {
        return Err(BatchValidationError::TooManyItems);
    }
    if encoded_json_size(&BatchEnvelope { kind, value: items })? > MAX_BATCH_BYTES {
        return Err(BatchValidationError::TooManyBytes);
    }
    Ok(())
}

impl<Body: Serialize + BatchRequest> RequestBatch<Body> {
    pub fn new(items: Vec<BatchItem<Body>>) -> Result<Self, BatchValidationError> {
        validate_size("requestBatch", &items)?;
        if let Some(first) = items.first()
            && items
                .iter()
                .skip(1)
                .any(|item| !first.body.same_request_kind(&item.body))
        {
            return Err(BatchValidationError::MixedRequestKinds);
        }
        Ok(Self(items))
    }
}

impl<Body: Serialize> AnswerBatch<Body> {
    pub fn new(items: Vec<BatchItem<Body>>) -> Result<Self, BatchValidationError> {
        validate_size("answerBatch", &items)?;
        Ok(Self(items))
    }
}

macro_rules! batch_accessors {
    ($batch:ident) => {
        impl<Body> $batch<Body> {
            pub fn items(&self) -> &[BatchItem<Body>] {
                &self.0
            }
            pub fn into_items(self) -> Vec<BatchItem<Body>> {
                self.0
            }
        }
    };
}
batch_accessors!(RequestBatch);
batch_accessors!(AnswerBatch);

impl<'de, Body: Serialize + DeserializeOwned + BatchRequest> Deserialize<'de>
    for RequestBatch<Body>
{
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::new(Vec::<BatchItem<Body>>::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

impl<'de, Body: Serialize + DeserializeOwned> Deserialize<'de> for AnswerBatch<Body> {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::new(Vec::<BatchItem<Body>>::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}
