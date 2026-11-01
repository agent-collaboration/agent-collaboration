//! Controlled observation of real client signing and introspection transport.
use crate::{
    ClientAssertionLifetime, Ed25519ClientSigner, PersonIntrospectionClient,
    PersonSessionConfirmer, PrivateKeyJwtClient, ServiceClientId, ServiceFormTransport,
    identity_contract::ResourceUri, organization_authority::PersonSessionConfirmation,
};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const CLIENT_AUTH_PROBE_PATH: &str = "/__conformance/client-authentication";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAuthenticationProbe {
    pub now_millis: i64,
    pub bearer_token: String,
    pub attempts: u16,
    pub first_id: message_board::RecordId,
}
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientAuthenticationObservation {
    Active,
    Inactive,
    Unavailable,
}

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}
struct ScriptedIds {
    first: u128,
    offset: AtomicU64,
}
impl IdSource for ScriptedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        // Fixed, injected fixture sequence; no wall clock or random ID source.
        durable_steps::Uuid::from_u128(
            self.first + u128::from(self.offset.fetch_add(1, Ordering::SeqCst)),
        )
    }
}

pub async fn observe_client_authentication<
    Signer: Ed25519ClientSigner,
    Transport: ServiceFormTransport,
>(
    signer: Signer,
    transport: Transport,
    client_id: ServiceClientId,
    endpoint: ResourceUri,
    input: ClientAuthenticationProbe,
) -> Result<Vec<ClientAuthenticationObservation>, crate::ClientAssertionRefusal> {
    let client = controlled_client(signer, client_id, input.now_millis, &input.first_id)?;
    let confirmation = PersonIntrospectionClient::new(client, transport, endpoint);
    let mut outcomes = Vec::with_capacity(usize::from(input.attempts));
    for _ in 0..input.attempts {
        outcomes.push(
            match confirmation
                .confirm_person_session(&input.bearer_token)
                .await
            {
                Ok(PersonSessionConfirmation::Active { .. }) => {
                    ClientAuthenticationObservation::Active
                }
                Ok(PersonSessionConfirmation::Inactive {}) => {
                    ClientAuthenticationObservation::Inactive
                }
                Err(_) => ClientAuthenticationObservation::Unavailable,
            },
        );
    }
    Ok(outcomes)
}

pub(crate) fn controlled_client<Signer: Ed25519ClientSigner>(
    signer: Signer,
    client_id: ServiceClientId,
    now_millis: i64,
    first_id: &message_board::RecordId,
) -> Result<PrivateKeyJwtClient<Signer>, crate::ClientAssertionRefusal> {
    let first = durable_steps::Uuid::parse_str(first_id.as_str())
        .map_err(|_| crate::ClientAssertionRefusal::InvalidId)?;
    PrivateKeyJwtClient::new(
        client_id,
        signer,
        Arc::new(FixedClock(now_millis)),
        Arc::new(ScriptedIds {
            first: first.as_u128(),
            offset: AtomicU64::new(0),
        }),
        ClientAssertionLifetime::from_seconds(60)?,
    )
}
