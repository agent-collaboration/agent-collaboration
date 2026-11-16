//! N9 tap cases that must fail until their deliverable lands.

use super::{BehaviorDefect, Deliverable, ExpectedFailure, RecordKey, SchemaDefect};
use crate::node_taps::TapDirection;

/// N9 hand-builds its `session/new` result `_meta.sessionProfile` with `historyUnavailable`
/// (provider_acp_session_route.rs:165), a field the owning `SessionResponseProfileMetadata`
/// (session_profile_codec.rs:564, `deny_unknown_fields`) does not declare, so the owning codec
/// rejects the value N9 sends. Shown since correction 3 holds owned `_meta` to its owner.
const SESSION_RESULT_META_UNKNOWN_FIELD: SchemaDefect = SchemaDefect {
    deliverable: Deliverable::D2,
    record: RecordKey {
        direction: TapDirection::Produced,
        kind: "response:session/new",
        ordinal: 0,
    },
    schema_path: "SessionResultMeta",
    instance_path: "/result/_meta",
    keyword: "routerCodec",
    detail: "",
};

pub(super) const ENTRIES: &[ExpectedFailure] = &[
    ExpectedFailure {
        case_id: "N9/usage_item_rendered_as_agent_text",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "N9 renders metadata items (Usage, config, info, commands) as agent message chunks; it must share N3's metadata filter",
        }),
        schema: &[SESSION_RESULT_META_UNKNOWN_FIELD],
    },
    ExpectedFailure {
        case_id: "N9/session_new_request",
        behavior: None,
        schema: &[SESSION_RESULT_META_UNKNOWN_FIELD],
    },
    ExpectedFailure {
        case_id: "N9/agent_message_session_update",
        behavior: None,
        schema: &[SESSION_RESULT_META_UNKNOWN_FIELD],
    },
    ExpectedFailure {
        case_id: "N9/session_prompt_request",
        behavior: None,
        schema: &[SESSION_RESULT_META_UNKNOWN_FIELD],
    },
];
