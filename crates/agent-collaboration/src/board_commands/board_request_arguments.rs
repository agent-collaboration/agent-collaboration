//! Caller-controlled request identities for declared writes.
use clap::Args;
use collaboration_client::board::RequestId;
#[derive(Args, Default)]
pub(super) struct RequestMetadataArguments {
    /// Reuse the original UUIDv7 only when retrying the same semantic request.
    #[arg(long)]
    pub request_id: Option<String>,
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
}
