//! N1 tap cases that must fail until their deliverable lands.

use super::{BehaviorDefect, Deliverable, ExpectedFailure, RecordKey, SchemaDefect};
use crate::node_taps::TapDirection;

pub(super) const ENTRIES: &[ExpectedFailure] = &[
    ExpectedFailure {
        case_id: "N1/unlisted_request_method_not_found",
        behavior: Some(BehaviorDefect {
            deliverable: Deliverable::CliProject,
            defect: "49 pinned client requests get \"method not found\"; every request must get Handled, Refused, Minimal or Ignored (R10)",
        }),
        schema: &[],
    },
    // No `config/write` entry: the method violation is on the test's own CLI frame
    // (SyntheticApparatus, correction 3 item G). N1's real method-table defect (it names
    // `config/write` and misses `config/value/write`, router_session_app_server_methods.rs:87) is
    // an inventory row fixed by D1.
    ExpectedFailure {
        case_id: "N1/approval_reply_to_n7",
        behavior: None,
        // N1 answers thread/start with a hand-built Thread that lacks the required projectId.
        schema: &[SchemaDefect {
            deliverable: Deliverable::D1,
            record: RecordKey {
                direction: TapDirection::Produced,
                kind: "response:thread/start",
                ordinal: 0,
            },
            schema_path: "/definitions/v2/Thread/required",
            instance_path: "/thread",
            keyword: "required",
            detail: "projectId",
        }],
    },
];
