//! Issuer-key refresh policy uses only the configured source, never JWT claims.
use crate::{IssuerKeyCache, IssuerKeyId, IssuerKeySet, IssuerVerificationKey};
use fleet_contracts::{IssuerKeyRefusal, IssuerKeyResolutionRefusal};
use fleet_runtime_ports::{Clock, IssuerKeySource};

pub async fn resolve_issuer_key<Source: IssuerKeySource>(
    source: &Source,
    cache: &IssuerKeyCache,
    clock: &dyn Clock,
    key_id: &IssuerKeyId,
) -> Result<IssuerVerificationKey, IssuerKeyResolutionRefusal> {
    if let Some(keys) = cache.fresh(source.configured_source(), receipt_time(clock)?)? {
        match keys.key(key_id) {
            Ok(key) => return Ok(key.clone()),
            Err(IssuerKeyRefusal::UnknownKey) => {}
            Err(refusal) => return Err(refusal.into()),
        }
    }
    // A cold fetch is already this request's one attempt at the current set.
    // A cached miss forces one refresh; neither path fetches a second time.
    let keys = fetch_keys(source, cache, clock).await?;
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

fn receipt_time(
    clock: &dyn Clock,
) -> Result<chrono::DateTime<chrono::Utc>, IssuerKeyResolutionRefusal> {
    chrono::DateTime::from_timestamp_millis(clock.now().unix_millis())
        .ok_or(IssuerKeyResolutionRefusal::InvalidClock)
}

#[cfg(test)]
#[path = "issuer_key_resolution_test.rs"]
mod tests;
