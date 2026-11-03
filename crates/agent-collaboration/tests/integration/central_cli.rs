//! Validation/help must complete without Router discovery, cloudflared or HTTP.
use std::process::Command;
#[test]
fn central_requires_its_explicit_url_before_any_discovery() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["central", "copies", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--central")
    );
    assert!(output.stderr.is_empty());
}
#[test]
fn central_command_help_exposes_the_owner_family_without_network() {
    for args in [
        vec!["central", "--help"],
        vec!["central", "project", "takeover", "confirm", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("--central"));
        if args.contains(&"confirm") {
            assert!(text.contains("--preview"));
        }
    }
}
