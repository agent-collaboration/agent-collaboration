//! Rejected-tail content stays protocol-owned, without board dependencies.
use crate::{DigestError, Epoch, PieceDigest, ProjectId, RequestId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TailDigest(PieceDigest);
impl TailDigest {
    pub fn new(value: String) -> Result<Self, DigestError> {
        PieceDigest::try_from(value).map(Self)
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TailReportRow {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub request_ids: Vec<RequestId>,
    pub digest: TailDigest,
}
