//! Client-authenticated confirmation at the pinned introspection endpoint.
use crate::{
    Ed25519ClientSigner, PersonSessionConfirmer, PrivateKeyJwtClient, ServiceForm,
    identity_contract::ResourceUri,
    organization_authority::{
        PersonSessionConfirmation, SignInUnavailable, decode_person_session_confirmation,
    },
};
use std::future::Future;

pub trait ServiceFormTransport: Send + Sync {
    fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> impl Future<Output = Result<Vec<u8>, ServiceFormFailure>> + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ServiceFormFailure {
    #[error("the service could not be reached")]
    Unreachable,
    #[error("the service refused the request")]
    Refused { status: u16 },
}

pub struct PersonIntrospectionClient<Signer, Transport> {
    client: PrivateKeyJwtClient<Signer>,
    transport: Transport,
    endpoint: ResourceUri,
}
impl<Signer, Transport> PersonIntrospectionClient<Signer, Transport> {
    #[must_use]
    pub fn new(
        client: PrivateKeyJwtClient<Signer>,
        transport: Transport,
        endpoint: ResourceUri,
    ) -> Self {
        Self {
            client,
            transport,
            endpoint,
        }
    }
}
impl<Signer: Ed25519ClientSigner, Transport: ServiceFormTransport> PersonSessionConfirmer
    for PersonIntrospectionClient<Signer, Transport>
{
    async fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> Result<PersonSessionConfirmation, SignInUnavailable> {
        let authentication = self
            .client
            .authenticate_at(&self.endpoint)
            .await
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?;
        let body = self
            .transport
            .post_service_form(
                &self.endpoint,
                &authentication.introspection_form(bearer_token),
            )
            .await
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)?;
        decode_person_session_confirmation(&body)
            .map_err(|_| SignInUnavailable::ConfirmationUnreachable)
    }
}
