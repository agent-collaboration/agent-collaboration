//! N5 tap cases that must fail until their deliverable lands.

use super::{BehaviorDefect, Deliverable, ExpectedFailure};

pub(super) const ENTRIES: &[ExpectedFailure] = &[
    ExpectedFailure {
        case_id: "N5/usage_payload_json_text",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "usage arrives as a JSON string inside SessionItem.text; needs a typed variant",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N5/config_options_payload_json_text",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "config options arrive as a JSON string inside SessionItem.text; needs a typed variant",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N5/session_info_payload_json_text",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "session info arrives as a JSON string inside SessionItem.text; needs a typed variant",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N5/available_commands_payload_json_text",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "available commands arrive as a JSON string inside SessionItem.text; needs a typed variant",
        }),
        schema: &[],
    },
];
