use std::{path::Path, process::Command};

#[test]
fn central_automation_limits_are_in_the_compiled_cli_help() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let runner = repository.join("scripts/cargo_debug_signing_runner.sh");
    for arguments in [
        ["wake", "send", "--help"],
        ["schedule", "create", "--help"],
        ["schedule", "update", "--help"],
        ["instruction", "create", "--help"],
        ["instruction", "update", "--help"],
    ] {
        let output = Command::new(&runner)
            .arg(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .current_dir(repository)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        let help = String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for limit in collaboration_client::protocol::limits::INVENTORY {
            let bounds = format!(
                "{}: {}..={}, step {}",
                limit.name, limit.minimum, limit.maximum, limit.step
            );
            assert!(help.contains(&bounds), "{arguments:?} omitted {bounds}");
        }
        assert!(help.contains("Replacement requires replace=true"));
        if arguments.first() == Some(&"wake") {
            assert!(help.contains("--first-due"));
        }
    }
}

#[test]
fn automation_file_byte_limits_precede_utf8_decoding() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let instruction_file = directory.path().join("oversized-instruction.txt");
    std::fs::write(&instruction_file, "🦁".repeat(262_145)).unwrap();
    let target = r#"{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},"sessionId":"fixture"}"#;
    for (arguments, limit) in [
        (vec!["instruction", "create"], "INSTRUCTION_MAX_BYTES"),
        (
            vec![
                "wake",
                "send",
                "--to",
                target,
                "--human-user",
                "--at",
                "2026-10-31T10:05:00Z",
            ],
            "WAKE_MESSAGE_MAX_BYTES",
        ),
    ] {
        let output = Command::new(repository.join("scripts/cargo_debug_signing_runner.sh"))
            .arg(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .arg("--text-file")
            .arg(&instruction_file)
            .arg("--service-directory")
            .arg(directory.path())
            .arg("--json")
            .current_dir(repository)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let response = String::from_utf8(output.stdout).unwrap();
        assert!(response.contains("limitExceeded"), "{response}");
        assert!(response.contains(limit), "{response}");
    }
}
