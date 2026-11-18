//! The executable reads receipts through the SDK and the real served API.
// This case uses the Fleet-only portion of the shared fixture.
use message_board::{ProjectId, RequestId};
use std::process::Command;

#[test]
fn request_effects_cli_requires_exactly_one_explicit_scope() {
    let request = RequestId::generate();
    let project = ProjectId::generate();
    for selectors in [
        vec![],
        vec![
            "--project-id",
            project.as_str(),
            "--workspace-id",
            project.as_str(),
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(["request-effects", request.as_str()])
            .args(selectors)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}
