//! Fresh RFC7523 client authentication, distinct from a C7 bearer credential.
use crate::{ServiceClientId, identity_contract::ResourceUri};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use runtime_primitives::{Clock, IdSource};
use serde::Serialize;
use std::{future::Future, sync::Arc};

/// One immutable Ed25519 key handle binds its published kid and signing bytes.
/// Hosts import it from their configured secret source; raw keys never enter
/// the shared caller or a request body. Better Auth1.7.11 permits EdDSA.
pub trait Ed25519ClientSigner: Send + Sync {
    fn key_id(&self) -> &str;
    fn sign_client_assertion(
        &self,
        signed: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, ClientAssertionRefusal>> + Send;
}

#[derive(Clone, Copy)]
pub struct ClientAssertionLifetime(u32);
impl ClientAssertionLifetime {
    pub fn from_seconds(seconds: u32) -> Result<Self, ClientAssertionRefusal> {
        // The pinned provider's assertionMaxLifetime default is300s. A host
        // supplies its chosen shorter TTL; this never extends C7 token life.
        if seconds == 0 || seconds > 300 {
            return Err(ClientAssertionRefusal::InvalidConfiguration);
        }
        Ok(Self(seconds))
    }
}

pub struct PrivateKeyJwtClient<Signer> {
    client_id: ServiceClientId,
    signer: Signer,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    lifetime: ClientAssertionLifetime,
}
impl<Signer: Ed25519ClientSigner> PrivateKeyJwtClient<Signer> {
    pub fn new(
        client_id: ServiceClientId,
        signer: Signer,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
        lifetime: ClientAssertionLifetime,
    ) -> Result<Self, ClientAssertionRefusal> {
        if signer.key_id().is_empty() {
            return Err(ClientAssertionRefusal::InvalidConfiguration);
        }
        Ok(Self {
            client_id,
            signer,
            clock,
            ids,
            lifetime,
        })
    }

    /// Never cache assertions: each attempt gets one injected UUIDv7 jti.
    pub async fn authenticate_at(
        &self,
        endpoint: &ResourceUri,
    ) -> Result<ClientAssertionAuthentication, ClientAssertionRefusal> {
        let issued_at = self.clock.now().unix_millis().div_euclid(1_000);
        if issued_at < 0 {
            return Err(ClientAssertionRefusal::InvalidClock);
        }
        let expires_at = issued_at
            .checked_add(i64::from(self.lifetime.0))
            .ok_or(ClientAssertionRefusal::InvalidClock)?;
        let token_id = self.ids.new_id();
        if token_id.get_version_num() != 7 {
            return Err(ClientAssertionRefusal::InvalidId);
        }
        let header = encode(&AssertionHeader {
            alg: "EdDSA",
            typ: "JWT",
            kid: self.signer.key_id(),
        })?;
        let claims = encode(&AssertionClaims {
            iss: self.client_id.as_str(),
            sub: self.client_id.as_str(),
            aud: endpoint.as_str(),
            iat: issued_at,
            exp: expires_at,
            jti: token_id.to_string(),
        })?;
        let signed = format!("{header}.{claims}");
        let signature = self.signer.sign_client_assertion(signed.as_bytes()).await?;
        if signature.len() != 64 {
            return Err(ClientAssertionRefusal::Signing);
        }
        Ok(ClientAssertionAuthentication {
            client_id: self.client_id.clone(),
            assertion: format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)),
        })
    }
}

/// No Debug or Serialize: the transport sees this credential only as a form.
pub struct ClientAssertionAuthentication {
    client_id: ServiceClientId,
    assertion: String,
}
impl ClientAssertionAuthentication {
    #[must_use]
    pub fn service_token_form(
        &self,
        resource: &ResourceUri,
        operation: crate::ServiceOperation,
    ) -> ServiceForm {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "client_credentials")
            .append_pair("resource", resource.as_str())
            .append_pair("scope", operation.scope())
            .append_pair("client_id", self.client_id.as_str())
            .append_pair(
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            )
            .append_pair("client_assertion", &self.assertion);
        ServiceForm(form.finish())
    }
    /// RFC7662 plus RFC7523 client authentication; the person token remains the
    /// token form field, never an Authorization bearer credential for this call.
    #[must_use]
    pub fn introspection_form(&self, person_token: &str) -> ServiceForm {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("token", person_token)
            .append_pair("client_id", self.client_id.as_str())
            .append_pair(
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            )
            .append_pair("client_assertion", &self.assertion);
        ServiceForm(form.finish())
    }
}

/// Secret-bearing request bytes with an explicit transport-only accessor.
pub struct ServiceForm(String);
impl ServiceForm {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Serialize)]
struct AssertionHeader<'key> {
    alg: &'static str,
    typ: &'static str,
    kid: &'key str,
}
#[derive(Serialize)]
struct AssertionClaims<'client> {
    iss: &'client str,
    sub: &'client str,
    aud: &'client str,
    iat: i64,
    exp: i64,
    jti: String,
}
fn encode(value: &impl Serialize) -> Result<String, ClientAssertionRefusal> {
    serde_json::to_vec(value)
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        .map_err(|_| ClientAssertionRefusal::Encoding)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClientAssertionRefusal {
    #[error("client assertion configuration is invalid")]
    InvalidConfiguration,
    #[error("client assertion clock is unavailable")]
    InvalidClock,
    #[error("client assertion ID source did not provide UUIDv7")]
    InvalidId,
    #[error("client assertion encoding failed")]
    Encoding,
    #[error("client assertion signing failed")]
    Signing,
}
