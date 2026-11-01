//! The C8 first-token routing extension belongs to this Host's OAuth response boundary.
use codex_router_fleet_state::fleet_login::SavedFleetCentral;
use fleet_sync_protocol::{PeerId, ReceiverId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearnedFleetCentral {
    pub receiver: ReceiverId,
    pub relay: PeerId,
}
impl From<LearnedFleetCentral> for SavedFleetCentral {
    fn from(value: LearnedFleetCentral) -> Self {
        Self {
            receiver: value.receiver,
            relay: value.relay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LearnedFleetCentral;
    use serde_json::json;

    // Wrong behavior: keeping the retired per-organization addresses or ignoring
    // unknown routing fields. Oracle: PD cbda512d's closed receiver/relay answer.
    // No pre-fix red observed for this updated two-service response.
    #[test]
    fn fleet_machine_login_central_answer_has_only_receiver_and_relay() {
        // Arrange
        let answer = json!({
            "receiver": "019f0000-0000-7000-8000-000000000011",
            "relay": "019f0000-0000-7000-8000-000000000012"
        });

        // Act
        let parsed = serde_json::from_value::<LearnedFleetCentral>(answer.clone()).unwrap();

        // Assert
        assert_eq!(serde_json::to_value(parsed).unwrap(), answer);
        for field in ["session_endpoint", "machine_address", "unexpected"] {
            let mut retired = answer.clone();
            retired[field] = json!("https://central.example.invalid/");
            assert!(serde_json::from_value::<LearnedFleetCentral>(retired).is_err());
        }
        for field in ["receiver", "relay"] {
            let mut missing = answer.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<LearnedFleetCentral>(missing).is_err());
        }
    }
}
