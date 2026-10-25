//! A backend generation carried by mail, independent of the Router RPC facade.
use message_board::ServiceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("generation must be a positive JSON-safe integer")]
pub struct GenerationGuardError;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationGuard {
    #[schemars(
        with = "String",
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    service_epoch: ServiceId,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    generation: u64,
}
impl GenerationGuard {
    pub fn new(service_epoch: ServiceId, generation: u64) -> Result<Self, GenerationGuardError> {
        if !(1..=9_007_199_254_740_991).contains(&generation) {
            return Err(GenerationGuardError);
        }
        Ok(Self {
            service_epoch,
            generation,
        })
    }
    #[must_use]
    pub fn service_epoch(&self) -> &ServiceId {
        &self.service_epoch
    }
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
impl<'de> Deserialize<'de> for GenerationGuard {
    fn deserialize<TDeserializer: serde::Deserializer<'de>>(
        deserializer: TDeserializer,
    ) -> Result<Self, TDeserializer::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireGuard {
            service_epoch: ServiceId,
            generation: u64,
        }
        let wire = WireGuard::deserialize(deserializer)?;
        Self::new(wire.service_epoch, wire.generation).map_err(serde::de::Error::custom)
    }
}
