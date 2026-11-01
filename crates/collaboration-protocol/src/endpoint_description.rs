//! Endpoint metadata keeps destination identity separate from protocol channels.
use crate::{
    CodexGeneration, EndpointRef, GenerationNumber, NonEmptyText, ObservationTimestamp,
    ProviderBindingId, ProviderCapabilities, ProviderRuntimeIdentity, ProviderTransport,
    SchemaDigest,
};
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum EndpointAvailability {
    #[serde(rename_all = "camelCase")]
    Available {
        observed_at: ObservationTimestamp,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        observed_at: ObservationTimestamp,
        reason: NonEmptyText,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fix: Option<NonEmptyText>,
    },
    Unprobed,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NativeCarrier {
    #[serde(rename = "unixWebSocket")]
    UnixWebSocket,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AcpCarrier {
    #[serde(rename = "unixJsonLines")]
    UnixJsonLines,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ChannelDescription {
    #[serde(rename_all = "camelCase")]
    NativeCodex {
        transport: NativeCarrier,
        path: NonEmptyText,
        schema_digest: Option<SchemaDigest>,
        generation: Option<CodexGeneration>,
    },
    #[serde(rename_all = "camelCase")]
    Acp {
        transport: AcpCarrier,
        path: NonEmptyText,
        schema_digest: SchemaDigest,
    },
    #[serde(rename_all = "camelCase")]
    ExternalProvider {
        transport: ProviderTransport,
        binding_id: ProviderBindingId,
        binding_generation: GenerationNumber,
        runtime: ProviderRuntimeIdentity,
        capabilities: ProviderCapabilities,
    },
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointDescription {
    pub endpoint: EndpointRef,
    pub label: NonEmptyText,
    pub availability: EndpointAvailability,
    #[schemars(length(min = 0, max = 2))]
    pub channels: Vec<ChannelDescription>,
}

impl<'de> Deserialize<'de> for EndpointDescription {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct EndpointDocument {
            endpoint: EndpointRef,
            label: NonEmptyText,
            availability: EndpointAvailability,
            channels: Vec<ChannelDescription>,
        }
        let document = EndpointDocument::deserialize(deserializer)?;
        if document.channels.len() > 2
            || (document.channels.is_empty()
                && !matches!(
                    document.availability,
                    EndpointAvailability::Unavailable { .. }
                ))
        {
            return Err(serde::de::Error::custom(
                "endpoint channel count does not match availability",
            ));
        }
        Ok(Self {
            endpoint: document.endpoint,
            label: document.label,
            availability: document.availability,
            channels: document.channels,
        })
    }
}
