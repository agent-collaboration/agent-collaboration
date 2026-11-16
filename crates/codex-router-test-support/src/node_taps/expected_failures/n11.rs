//! N11 tap cases that must fail until their deliverable lands.

use super::{BehaviorDefect, Deliverable, ExpectedFailure};

pub(super) const ENTRIES: &[ExpectedFailure] = &[ExpectedFailure {
    case_id: "N11/session_command_tool_to_n4",
    behavior: Some(BehaviorDefect {
        // Owner ruling (addendum 2d): prompt admission is the CLI+MCP lane's, not schema work.
        deliverable: Deliverable::External("cli-mcp-lane"),
        defect: "N11's conversation operations call the provider supervisor directly, bypassing N4",
    }),
    schema: &[],
}];
