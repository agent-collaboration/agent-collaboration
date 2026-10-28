//! The core owns key lifetime; runtimes retain this value across requests.

use crate::{AccessJwks, AccessRefusal, RawJwks, RegistryTime};
use std::sync::Mutex;

#[derive(Default)]
pub struct AccessKeyCache {
    cached: Mutex<Option<CachedAccessKeys>>,
}

struct CachedAccessKeys {
    source: String,
    keys: AccessJwks,
    expires: RegistryTime,
}

impl AccessKeyCache {
    pub(crate) fn fresh(
        &self,
        source: &str,
        now: RegistryTime,
    ) -> Result<Option<AccessJwks>, AccessRefusal> {
        let cached = self
            .cached
            .lock()
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        Ok(cached
            .as_ref()
            .filter(|entry| entry.source == source && now.0 < entry.expires.0)
            .map(|entry| entry.keys.clone()))
    }

    pub(crate) fn remember(
        &self,
        source: String,
        response: RawJwks,
        now: RegistryTime,
    ) -> Result<AccessJwks, AccessRefusal> {
        let keys: AccessJwks = serde_json::from_slice(&response.body)
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        let max_age = response
            .cache_control
            .as_deref()
            .and_then(|header| {
                header
                    .split(',')
                    .find_map(|part| part.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
            })
            .unwrap_or(3600)
            .min(3600);
        let expires = now
            .0
            .checked_add_signed(chrono::TimeDelta::seconds(
                i64::try_from(max_age).map_err(|_| AccessRefusal::KeySourceUnavailable)?,
            ))
            .ok_or(AccessRefusal::KeySourceUnavailable)?;
        *self
            .cached
            .lock()
            .map_err(|_| AccessRefusal::KeySourceUnavailable)? = Some(CachedAccessKeys {
            source,
            keys: keys.clone(),
            expires: RegistryTime(expires),
        });
        Ok(keys)
    }
}
