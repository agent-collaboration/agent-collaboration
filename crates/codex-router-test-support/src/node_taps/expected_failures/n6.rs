//! N6 tap cases that must fail until their deliverable lands.

use super::{BehaviorDefect, Deliverable, ExpectedFailure};

pub(super) const ENTRIES: &[ExpectedFailure] = &[
    ExpectedFailure {
        case_id: "N6/config_option_update",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "ConfigChange carries typed config options, not a JSON string",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N6/session_info_update",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "SessionInfo carries typed fields, not a JSON string",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N6/usage_update",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "Usage { used, size } typed, not a JSON string",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N6/available_commands_update",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "CommandCatalog typed (today a Notice whose text is JSON)",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N6/available_commands_update_with_skill_view",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "CommandCatalog typed (today a Notice whose text is JSON); skill-view update unchanged",
        }),
        schema: &[],
    },
    ExpectedFailure {
        case_id: "N6/steering_started_new_turn",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "startedNewTurn while idle publishes TurnStarted and the user item, then at once TurnEnded(Lost: EndNotObservable): the turn is reported lost while the agent still answers",
        }),
        schema: &[],
    },
];
