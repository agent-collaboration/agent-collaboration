//! Decode the producer-owned K5 reply, then discard its remote checkedAt clock.

use super::{
    person_confirmation::{PersonConfirmationRefusal, PersonSessionConfirmation},
    person_token::SignInSessionRef,
    person_wire_values::{PersonWireSeconds, ProviderCheckTime},
};
use crate::identity_contract::PersonId;
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Deserializer};

pub fn decode_person_session_confirmation(
    body: &[u8],
) -> Result<PersonSessionConfirmation, PersonConfirmationRefusal> {
    let wire: ConfirmationWire =
        serde_json::from_slice(body).map_err(|_| PersonConfirmationRefusal::MalformedReply)?;
    match wire {
        ConfirmationWire::Inactive(_) => Ok(PersonSessionConfirmation::Inactive {}),
        ConfirmationWire::Active(active) => {
            if url::Url::parse(&active.audience).is_err() {
                return Err(PersonConfirmationRefusal::MalformedReply);
            }
            Ok(PersonSessionConfirmation::Active {
                person_id: active.person_id,
                session_ref: active.session_ref,
                audience: active.audience,
                organization_id: active.organization_id,
                expires_at: active.expires_at.into_unix_seconds(),
            })
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ConfirmationWire {
    Inactive(InactiveWire),
    Active(ActiveWire),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InactiveWire {
    #[serde(rename = "active")]
    _state: ConfirmationFlag<false>,
}

#[derive(Deserialize)]
struct ActiveWire {
    #[serde(rename = "active")]
    _state: ConfirmationFlag<true>,
    #[serde(rename = "sub")]
    person_id: PersonId,
    #[serde(rename = "sid")]
    session_ref: SignInSessionRef,
    #[serde(rename = "aud")]
    audience: String,
    organization_id: OrganizationId,
    #[serde(rename = "exp")]
    expires_at: PersonWireSeconds,
    // This validates the field, but cannot supply time to the central guard.
    #[serde(rename = "checkedAt")]
    _checked_at: ProviderCheckTime,
}

struct ConfirmationFlag<const ACTIVE: bool>;

impl<'input, const ACTIVE: bool> Deserialize<'input> for ConfirmationFlag<ACTIVE> {
    fn deserialize<Decoder: Deserializer<'input>>(
        decoder: Decoder,
    ) -> Result<Self, Decoder::Error> {
        if bool::deserialize(decoder)? != ACTIVE {
            return Err(serde::de::Error::custom("unexpected confirmation state"));
        }
        Ok(Self)
    }
}
