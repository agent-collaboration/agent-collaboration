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
#[derive(Default)]
struct ScriptedIds(AtomicU64);
impl IdSource for ScriptedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        // Fixed, injected fixture sequence; no wall clock or random ID source.
        durable_steps::Uuid::from_u128(
            0x019f0000000070008000000000000000 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
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
    let client = PrivateKeyJwtClient::new(
        client_id,
        signer,
        Arc::new(FixedClock(input.now_millis)),
        Arc::new(ScriptedIds::default()),
        ClientAssertionLifetime::from_seconds(60)?,
    )?;
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
