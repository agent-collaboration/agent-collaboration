//! Delivery intent is independent of carrier selection and message authorship.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(JsonSchema, Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MailDeliveryMode {
    #[default]
    Auto,
    Queue,
    Steer,
}

#[cfg(test)]
mod tests {
    use super::MailDeliveryMode;

    #[test]
    fn delivery_intent_keeps_its_existing_wire_tags_and_closed_variants() {
        for (delivery, wire) in [
            (MailDeliveryMode::Auto, "auto"),
            (MailDeliveryMode::Queue, "queue"),
            (MailDeliveryMode::Steer, "steer"),
        ] {
            let encoded = serde_json::to_value(delivery).expect("delivery encoding");
            assert_eq!(encoded, wire);
            assert_eq!(
                serde_json::from_value::<MailDeliveryMode>(encoded).expect("delivery decoding"),
                delivery
            );
        }
        assert_eq!(MailDeliveryMode::default(), MailDeliveryMode::Auto);
        assert!(serde_json::from_str::<MailDeliveryMode>(r#""defer""#).is_err());
        assert!(serde_json::from_str::<MailDeliveryMode>(r#"{"kind":"auto"}"#).is_err());
    }
}
