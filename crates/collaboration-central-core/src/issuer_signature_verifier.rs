//! Exact signed bytes are checked against keys from the configured issuer source.
use crate::{
    CentralPlatform, IssuerKeyCache, IssuerKeyId, IssuerKeyRefusal, IssuerKeyResolutionRefusal,
    IssuerKeySource, SignatureValidity, VerifyFailure, resolve_issuer_key,
};
use runtime_primitives::Clock;
use std::sync::Arc;

pub struct IssuerSignatureVerifier<Platform, Source> {
    platform: Platform,
    source: Source,
    keys: Arc<IssuerKeyCache>,
    clock: Arc<dyn Clock>,
}

impl<Platform: CentralPlatform, Source: IssuerKeySource> IssuerSignatureVerifier<Platform, Source> {
    #[must_use]
    pub fn new(
        platform: Platform,
        source: Source,
        keys: Arc<IssuerKeyCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            platform,
            source,
            keys,
            clock,
        }
    }

    pub async fn verify(
        &self,
        key_id: &str,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, IssuerSignatureRefusal> {
        let key_id = match IssuerKeyId::try_from(key_id.to_owned()) {
            Ok(key_id) => key_id,
            Err(_) => return Ok(SignatureValidity::Invalid),
        };
        let key = match resolve_issuer_key(&self.source, &self.keys, self.clock.as_ref(), &key_id)
            .await
        {
            Ok(key) => key,
            Err(IssuerKeyResolutionRefusal::KeySet(IssuerKeyRefusal::UnknownKey)) => {
                return Ok(SignatureValidity::Invalid);
            }
            Err(refusal) => return Err(IssuerSignatureRefusal::Keys(refusal)),
        };
        self.platform
            .verify_ed25519(key.public_key(), signed, signature)
            .await
            .map_err(|reason| IssuerSignatureRefusal::Crypto { reason })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerSignatureRefusal {
    #[error(transparent)]
    Keys(#[from] IssuerKeyResolutionRefusal),
    #[error("the issuer-signature capability is unavailable")]
    Crypto { reason: VerifyFailure },
}
