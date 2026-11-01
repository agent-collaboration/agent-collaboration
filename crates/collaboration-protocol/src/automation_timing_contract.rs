//! Public timing inputs use bounded durations and the existing UTC timestamp contract.
use crate::ObservationTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct PositiveSeconds(u32);
impl PositiveSeconds {
    pub const DEFAULT_EXECUTION_TIMEOUT: Self = Self(3600);
    pub const DEFAULT_SUMMARY_TIMEOUT: Self = Self(900);
}
impl JsonSchema for PositiveSeconds {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PositiveSeconds".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","minimum":1,"maximum":31536000})
    }
}

#[derive(Debug, thiserror::Error)]
#[error("duration must be an integer between 1 and 31536000 seconds")]
pub struct InvalidSeconds;
impl TryFrom<u32> for PositiveSeconds {
    type Error = InvalidSeconds;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if (1..=31_536_000).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidSeconds)
        }
    }
}
impl From<PositiveSeconds> for u32 {
    fn from(value: PositiveSeconds) -> Self {
        value.0
    }
}
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TimingRequest {
    At {
        at: ObservationTimestamp,
    },
    After {
        seconds: PositiveSeconds,
    },
    Interval {
        seconds: PositiveSeconds,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_due: Option<ObservationTimestamp>,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}

pub(crate) fn central_timing_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let instant = schemars::json_schema!({"allOf":[generator.subschema_for::<ObservationTimestamp>()],
        "pattern":"T[0-9]{2}:(00|05|10|15|20|25|30|35|40|45|50|55):00(\\.0+)?Z$"});
    let period = generator.subschema_for::<crate::GridPeriod>();
    schemars::json_schema!({"oneOf":[
            {"type":"object","properties":{"kind":{"const":"at"},"at":instant},
                "required":["kind","at"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"interval"},"seconds":period,"firstDue":instant},
                "required":["kind","seconds"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"cron"},"expression":{"type":"string"},"timezone":{"type":"string"}},
                "required":["kind","expression","timezone"],"additionalProperties":false}],
            "description":crate::limits::description(),
            "x-automationLimits":generator.subschema_for::<crate::limits::AutomationLimits>()})
}

pub(crate) fn wake_timing_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"allOf":[central_timing_schema(generator)],
        "if":{"properties":{"kind":{"const":"interval"}}},
        "then":{"properties":{"seconds":{"minimum":crate::limits::WAKE_MIN_PERIOD}}}})
}
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExpiryRequest {
    None,
    At { at: ObservationTimestamp },
    After { seconds: PositiveSeconds },
}
