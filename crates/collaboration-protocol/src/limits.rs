//! Product limits shared by schemas, entry validation and caller descriptions.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

pub const GRID_STEP: u32 = 300;
pub const WAKE_MIN_PERIOD: u32 = 600;
pub const WAKE_ONE_SHOT_MAX_AHEAD: u32 = 86_400;
pub const WAKE_REPEAT_MAX_LIFETIME: u32 = 604_800;
pub const SCHEDULE_MIN_INTERVAL: u32 = GRID_STEP;
pub const NOTIFICATION_INTERVAL: u32 = 600;
pub const NOTIFICATION_LISTING_MAX_ENTRIES: usize = 50;
pub const BATCH_MAX_ENTRIES: usize = 100;
pub const REPORT_FLUSH_MAX: usize = 100;
pub const REPORT_FLUSH_WINDOW: u32 = 500;
pub const INSTRUCTION_MAX_BYTES: usize = agent_automation::InstructionText::MAX_BYTES;
pub const WAKE_MESSAGE_MAX_BYTES: usize = crate::MAX_CONTROL_FRAME_BYTES;
pub const ENDPOINT_ID_MAX_BYTES: usize = 64;
pub const SESSION_ID_MAX_BYTES: usize = 4096;
pub const TEXT_MAX_BYTES: usize = 4096;
pub const DURATION_MAX_SECONDS: u32 = 31_536_000;
pub const JSON_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitDefinition {
    pub name: LimitName,
    pub minimum: u64,
    pub maximum: u64,
    pub step: u64,
    pub unit: &'static str,
    pub description: &'static str,
}

// One declaration generates the inventory, names and exhaustive lookup. No index
// lookup can drift away from its limit, and every consumer reads the same bounds.
macro_rules! define_limits {
    ($($variant:ident => ($name:literal, $min:expr, $max:expr, $step:expr, $unit:literal, $description:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
        pub enum LimitName { $(#[serde(rename = $name)] $variant),+ }
        impl LimitName {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
            #[must_use]
            pub const fn definition(self) -> LimitDefinition {
                match self { $(Self::$variant => LimitDefinition {
                    name: self, minimum: $min as u64, maximum: $max as u64,
                    step: $step as u64, unit: $unit, description: $description,
                }),+ }
            }
            pub fn validate(self, value: u64) -> Result<(), LimitExceeded> {
                self.definition().validate(value)
            }
        }
        pub const INVENTORY: &[LimitDefinition] = &[$(LimitName::$variant.definition()),+];
    };
}

define_limits!(
    GridStep => ("GRID_STEP", GRID_STEP, GRID_STEP, GRID_STEP, "seconds", "Only UTC five-minute marks; no rounding or relative after timing."),
    WakeMinPeriod => ("WAKE_MIN_PERIOD", WAKE_MIN_PERIOD, DURATION_MAX_SECONDS, GRID_STEP, "seconds", "Repeating wakes are at least ten minutes apart, on the five-minute grid."),
    WakeOneShotMaxAhead => ("WAKE_ONE_SHOT_MAX_AHEAD", 0, WAKE_ONE_SHOT_MAX_AHEAD, 1, "seconds", "One-shot wakes are due at most 24 hours ahead."),
    WakeRepeatMaxLifetime => ("WAKE_REPEAT_MAX_LIFETIME", 1, WAKE_REPEAT_MAX_LIFETIME, 1, "seconds", "Repeating wakes end within seven days; the end need not be on the grid."),
    ScheduleMinInterval => ("SCHEDULE_MIN_INTERVAL", SCHEDULE_MIN_INTERVAL, DURATION_MAX_SECONDS, GRID_STEP, "seconds", "Schedule intervals are multiples of five minutes."),
    NotificationInterval => ("NOTIFICATION_INTERVAL", NOTIFICATION_INTERVAL, NOTIFICATION_INTERVAL, 1, "seconds", "At most one notification delivery per recipient in ten minutes; direct messages, wakes, runs, approvals and questions stay immediate."),
    NotificationListingMaxEntries => ("NOTIFICATION_LISTING_MAX_ENTRIES", 1, NOTIFICATION_LISTING_MAX_ENTRIES, 1, "entries", "A notification listing includes a count and pointer for overflow; no item is lost."),
    BatchMaxEntries => ("BATCH_MAX_ENTRIES", 1, BATCH_MAX_ENTRIES, 1, "entries", "A frozen machine batch holds at most 100 entries, with content by reference."),
    ReportFlushMax => ("REPORT_FLUSH_MAX", 1, REPORT_FLUSH_MAX, 1, "reports", "The hundredth report flushes immediately."),
    ReportFlushWindow => ("REPORT_FLUSH_WINDOW", REPORT_FLUSH_WINDOW, REPORT_FLUSH_WINDOW, 1, "milliseconds", "Queued reports flush 500 ms after the first queued report, or at 100 reports, whichever comes first."),
    InstructionMaxBytes => ("INSTRUCTION_MAX_BYTES", 1, INSTRUCTION_MAX_BYTES, 1, "UTF-8 bytes", "Instruction text is nonempty, NUL-free and at most one MiB."),
    WakeMessageMaxBytes => ("WAKE_MESSAGE_MAX_BYTES", 1, WAKE_MESSAGE_MAX_BYTES, 1, "UTF-8 bytes", "Wake text is nonempty and at most one MiB; only newline and tab C0 controls are allowed."),
    EndpointIdMaxBytes => ("ENDPOINT_ID_MAX_BYTES", 1, ENDPOINT_ID_MAX_BYTES, 1, "ASCII bytes", "Endpoint IDs start with a lowercase letter and contain lowercase letters, digits or hyphens."),
    SessionIdMaxBytes => ("SESSION_ID_MAX_BYTES", 1, SESSION_ID_MAX_BYTES, 1, "UTF-8 bytes", "Session IDs are nonempty and NUL-free."),
    TextMaxBytes => ("TEXT_MAX_BYTES", 1, TEXT_MAX_BYTES, 1, "UTF-8 bytes", "Identity descriptions are nonempty and NUL-free."),
    WakeSlot => ("WAKE_SLOT", 1, 1, 1, "wake per session", "Active and paused wakes occupy the slot. Replacement requires replace=true and the creator, target session or owner. An unrecorded creator permits only the target or owner."),
    JsonSafeInteger => ("JSON_SAFE_INTEGER", 1, JSON_SAFE_INTEGER_MAX, 1, "integer", "Generation and report sequence counters are positive JSON-safe integers."),
);

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[error("limitExceeded: {limit}: {value}; allowed {minimum}..={maximum}, step {step}")]
pub struct LimitExceeded {
    pub limit: LimitName,
    pub value: u64,
    pub minimum: u64,
    pub maximum: u64,
    pub step: u64,
}

impl LimitDefinition {
    pub fn validate(self, value: u64) -> Result<(), LimitExceeded> {
        if value < self.minimum || value > self.maximum || !value.is_multiple_of(self.step) {
            return Err(LimitExceeded {
                limit: self.name,
                value,
                minimum: self.minimum,
                maximum: self.maximum,
                step: self.step,
            });
        }
        Ok(())
    }
    #[must_use]
    pub fn schema(self) -> Schema {
        json_schema!({"type":"integer", "minimum":self.minimum,
            "maximum":self.maximum, "multipleOf":self.step,
            "description":self.description, "x-unit":self.unit})
    }
}

impl std::fmt::Display for LimitName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The complete contract is attached to automation tool schemas and CLI help.
#[must_use]
pub fn description() -> String {
    INVENTORY
        .iter()
        .map(|limit| {
            format!(
                "{}: {}..={}, step {} {}. {}",
                limit.name, limit.minimum, limit.maximum, limit.step, limit.unit, limit.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct AutomationLimits;
impl JsonSchema for AutomationLimits {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AutomationLimits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mut properties = serde_json::Map::new();
        for limit in INVENTORY {
            properties.insert(limit.name.as_str().into(), limit.schema().to_value());
        }
        json_schema!({"type":"object", "properties":properties,
            "description":description(), "additionalProperties":false})
    }
}
