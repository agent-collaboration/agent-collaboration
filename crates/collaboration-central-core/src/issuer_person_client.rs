//! Compose issuer signature checking with the independently authenticated live call.
use crate::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, IssuerSignatureVerifier,
    SignatureValidity,
    organization_authority::{
        PersonSessionConfirmation, PersonSignInPort, PersonSignatureAlgorithm,
        PersonSignatureInput, SignInUnavailable,
    },
};
use runtime_primitives::Clock;
use std::{future::Future, sync::Arc};

/// The service caller authenticates at introspection and decodes F2a's K5 type.
pub trait PersonSessionConfirmer: Send + Sync {
    fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> impl Future<Output = Result<PersonSessionConfirmation, SignInUnavailable>> + Send + 'request;
}

pub struct IssuerPersonSignInClient<Platform, Source, Confirmation> {
    signatures: IssuerSignatureVerifier<Platform, Source>,
    confirmation: Confirmation,
}

impl<Platform: Ed25519Verification, Source: IssuerKeySource, Confirmation: PersonSessionConfirmer>
    IssuerPersonSignInClient<Platform, Source, Confirmation>
{
    #[must_use]
    pub fn new(
        platform: Platform,
        source: Source,
        keys: Arc<IssuerKeyCache>,
        confirmation: Confirmation,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            signatures: IssuerSignatureVerifier::new(platform, source, keys, clock),
            confirmation,
        }
    }
}

impl<Platform: Ed25519Verification, Source: IssuerKeySource, Confirmation: PersonSessionConfirmer>
    PersonSignInPort for IssuerPersonSignInClient<Platform, Source, Confirmation>
{
    async fn verify_person_signature<'request>(
        &'request self,
        input: PersonSignatureInput<'request>,
    ) -> Result<SignatureValidity, SignInUnavailable> {
        match input.algorithm() {
            PersonSignatureAlgorithm::EdDsa => self
                .signatures
                .verify(input.key_id(), input.signed_bytes(), input.signature())
                .await
                // This is an unavailable verification source, never a guessed
                // Invalid verdict. Keep F2a's existing two-channel port contract.
                .map_err(|_| SignInUnavailable::KeysUnreachable),
        }
    }

    async fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> Result<PersonSessionConfirmation, SignInUnavailable> {
        self.confirmation.confirm_person_session(bearer_token).await
    }
}
