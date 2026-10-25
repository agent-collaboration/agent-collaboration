//! Central owns its deployment relay identity independently of lease policy.
use fleet_sync_protocol::PeerId;
use serde::{Deserialize, Serialize};

/// The configured relay identity returned to Routers at enrollment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryConfig {
    pub relay_peer: PeerId,
}

#[cfg(test)]
#[allow(
    clippy::panic_in_result_fn,
    reason = "Literal deployment-config assertions fail their test."
)]
mod tests {
    use super::*;
    #[test]
    fn relay_identity_is_required_validated_and_serialized_explicitly()
    -> Result<(), Box<dyn std::error::Error>> {
        let wire = serde_json::json!({"relayPeer":"019f0000-0000-7000-8000-000000005240"});
        let config: DeliveryConfig = serde_json::from_value(wire.clone())?;
        assert_eq!(
            config.relay_peer.to_string(),
            "019f0000-0000-7000-8000-000000005240"
        );
        assert_eq!(serde_json::to_value(config)?, wire);
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"relayPeer":null}),
            serde_json::json!({"relayPeer":"not-a-peer"}),
            serde_json::json!({"relayPeer":"550e8400-e29b-41d4-a716-446655440524"}),
            serde_json::json!({"relayPeer":"019f0000-0000-7000-8000-000000005240","authoritativeReceiver":"019f0000-0000-7000-8000-000000000010"}),
        ] {
            assert!(serde_json::from_value::<DeliveryConfig>(invalid).is_err());
        }
        Ok(())
    }
}
