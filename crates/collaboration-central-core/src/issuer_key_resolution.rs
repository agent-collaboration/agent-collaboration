//! Issuer-key refresh policy uses only the configured source, never JWT claims.
use crate::{
    IssuerKeyCache, IssuerKeyId, IssuerKeyRefusal, IssuerKeySet, IssuerVerificationKey,
    KeyFetchFailure, RawJwks, RegistryTime,
};
use runtime_primitives::Clock;
use std::future::Future;

/// A host binds this port to the issuer's key address from pinned configuration.
pub trait IssuerKeySource: Send + Sync {
    fn configured_source(&self) -> &str;
    fn fetch_issuer_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerKeyResolutionRefusal {
    #[error("the issuer's key source could not be reached")]
    Fetch { reason: KeyFetchFailure },
    #[error(transparent)]
    KeySet(#[from] IssuerKeyRefusal),
    #[error("the receiver's key-cache clock is unavailable")]
    InvalidClock,
}

pub async fn resolve_issuer_key<Source: IssuerKeySource>(
    source: &Source,
    cache: &IssuerKeyCache,
    clock: &dyn Clock,
    key_id: &IssuerKeyId,
) -> Result<IssuerVerificationKey, IssuerKeyResolutionRefusal> {
    let mut keys = match cache.fresh(source.configured_source(), receipt_time(clock)?)? {
        Some(keys) => keys,
        None => fetch_keys(source, cache, clock).await?,
    };
    if matches!(keys.key(key_id), Err(IssuerKeyRefusal::UnknownKey)) {
        keys = fetch_keys(source, cache, clock).await?;
    }
    keys.key(key_id).cloned().map_err(Into::into)
}

async fn fetch_keys<Source: IssuerKeySource>(
    source: &Source,
    cache: &IssuerKeyCache,
    clock: &dyn Clock,
) -> Result<IssuerKeySet, IssuerKeyResolutionRefusal> {
    let response = source
        .fetch_issuer_keys()
        .await
        .map_err(|reason| IssuerKeyResolutionRefusal::Fetch { reason })?;
    // The TTL starts after the response, on this receiver's injected clock.
    cache
        .remember(
            source.configured_source().to_owned(),
            response,
            receipt_time(clock)?,
        )
        .map_err(Into::into)
}

fn receipt_time(clock: &dyn Clock) -> Result<RegistryTime, IssuerKeyResolutionRefusal> {
    chrono::DateTime::from_timestamp_millis(clock.now().unix_millis())
        .map(RegistryTime)
        .ok_or(IssuerKeyResolutionRefusal::InvalidClock)
}
