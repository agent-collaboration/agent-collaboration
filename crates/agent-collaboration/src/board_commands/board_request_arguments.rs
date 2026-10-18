//! Caller-controlled identities and retry provenance for declared writes.
use clap::Args;
use collaboration_client::board::{EpochNumber, RequestId};
#[derive(Args, Default)]
pub(super) struct RequestMetadataArguments {
    /// Reuse the original UUIDv7 only when retrying the same semantic request.
    #[arg(long)]
    pub request_id: Option<String>,
    /// Writer epoch observed by this request.
    #[arg(long)]
    pub observed_epoch: Option<u64>,
    /// Original request replaced by this new request.
    #[arg(long)]
    pub resubmits: Option<String>,
}
impl RequestMetadataArguments {
    pub fn request_id(&self) -> Result<RequestId, String> {
        self.request_id
            .as_ref()
            .map(|value| {
                RequestId::try_from(value.clone())
                    .map_err(|_| "--request-id must be a canonical UUIDv7".to_owned())
            })
            .transpose()
            .map(|value| value.unwrap_or_else(RequestId::generate))
    }
    pub fn observed_epoch(&self) -> Result<Option<EpochNumber>, String> {
        self.observed_epoch
            .map(|value| {
                value
                    .try_into()
                    .map_err(|_| "--observed-epoch must be a valid writer epoch".to_owned())
            })
            .transpose()
    }
    pub fn resubmits(&self) -> Result<Option<RequestId>, String> {
        self.resubmits
            .as_ref()
            .map(|value| {
                value
                    .clone()
                    .try_into()
                    .map_err(|_| "--resubmits must be a canonical UUIDv7".to_owned())
            })
            .transpose()
    }
}
