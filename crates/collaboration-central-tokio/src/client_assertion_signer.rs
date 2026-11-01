//! Native client-key handle imported by the host from its secret source.
use collaboration_central_core::{ClientAssertionRefusal, Ed25519ClientSigner, IssuerKeyId};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::sync::Arc;

#[derive(Clone)]
pub struct TokioEd25519ClientSigner {
    key_id: IssuerKeyId,
    key: Arc<Ed25519KeyPair>,
}
impl TokioEd25519ClientSigner {
    pub fn from_pkcs8(key_id: IssuerKeyId, secret: &[u8]) -> Result<Self, ClientAssertionRefusal> {
        // Accept standard PKCS8 v1 (derive public key) or v2 (check its public
        // component too), matching WebCrypto's pkcs8 import format.
        let key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(secret)
            .map_err(|_| ClientAssertionRefusal::Signing)?;
        Ok(Self {
            key_id,
            key: Arc::new(key),
        })
    }
    #[must_use]
    pub fn public_key(&self) -> &[u8] {
        self.key.public_key().as_ref()
    }
}
impl Ed25519ClientSigner for TokioEd25519ClientSigner {
    fn key_id(&self) -> &str {
        self.key_id.as_str()
    }
    async fn sign_client_assertion(
        &self,
        signed: &[u8],
    ) -> Result<Vec<u8>, ClientAssertionRefusal> {
        Ok(self.key.sign(signed).as_ref().to_vec())
    }
}
