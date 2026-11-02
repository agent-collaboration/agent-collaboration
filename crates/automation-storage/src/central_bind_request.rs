//! One codec retains the exact typed Bind bytes; a retry never reconstructs their revision.
use crate::StorageError;
use collaboration_protocol::ScheduleBindPreparedRequest;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StoredCentralBindRequest(String);

impl StoredCentralBindRequest {
    pub fn encode(request: &ScheduleBindPreparedRequest) -> Result<Self, StorageError> {
        Self::try_from(serde_json::to_string(request).map_err(|_| StorageError::InvalidRecord)?)
    }
    pub fn request(&self) -> Result<ScheduleBindPreparedRequest, StorageError> {
        serde_json::from_str(&self.0).map_err(|_| StorageError::InvalidRecord)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for StoredCentralBindRequest {
    type Error = StorageError;
    fn try_from(encoded: String) -> Result<Self, Self::Error> {
        if encoded.len() > 1_048_576 {
            return Err(StorageError::InvalidRecord);
        }
        let request: ScheduleBindPreparedRequest =
            serde_json::from_str(&encoded).map_err(|_| StorageError::InvalidRecord)?;
        if !request.bound_thread.cwd.starts_with('/')
            || request.bound_thread.cwd.contains('\0')
            || serde_json::to_string(&request).map_err(|_| StorageError::InvalidRecord)? != encoded
        {
            return Err(StorageError::InvalidRecord);
        }
        Ok(Self(encoded))
    }
}
impl From<StoredCentralBindRequest> for String {
    fn from(value: StoredCentralBindRequest) -> Self {
        value.0
    }
}
