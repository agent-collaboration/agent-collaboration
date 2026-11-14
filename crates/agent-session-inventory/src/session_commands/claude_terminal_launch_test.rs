use super::*;

#[test]
fn background_receipt_accepts_recorded_named_and_unnamed_vendor_output() {
    for receipt in [
        "backgrounded · a1b2c3d4 · <redacted> (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
        "backgrounded · a1b2c3d4 (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
    ] {
        let handle = parse_background_receipt(receipt.as_bytes())
            .expect("recorded Claude2.1.312 background receipt");
        assert_eq!(String::from(handle), "a1b2c3d4");
    }
}

#[test]
fn background_receipt_does_not_guess_or_accept_ambiguous_handles() {
    for receipt in [
        "",
        "  claude attach a1b2c3d4 open in this terminal\n",
        "backgrounded · not-a-job · name\n",
        "backgrounded · a1b2c3d4 · one\nbackgrounded · deadbeef · two\n",
    ] {
        let message = parse_background_receipt(receipt.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(message.contains("claude agents") && message.contains("do not start another copy"));
    }
}

#[test]
fn resume_plans_preserve_every_argument_without_changing_native_mode() {
    let arguments = vec![
        OsString::from("--model"),
        OsString::from("haiku"),
        OsString::from("--name"),
        OsString::from("東京 --bg value"),
        OsString::from("--unknown-native-option"),
    ];
    let cases = [
        (
            ClaudeResumeMode::Foreground {
                session_id: "session".to_owned(),
            },
            vec![OsString::from("--resume"), OsString::from("session")],
        ),
        (
            ClaudeResumeMode::Attach {
                job_handle: "deadbeef".to_owned().try_into().unwrap(),
            },
            vec![OsString::from("attach"), OsString::from("deadbeef")],
        ),
        (
            ClaudeResumeMode::RestartBackground {
                session_id: "session".to_owned(),
            },
            vec![
                OsString::from("--bg"),
                OsString::from("--resume"),
                OsString::from("session"),
            ],
        ),
    ];
    for (mode, mut expected) in cases {
        let plan = ClaudeResumePlan {
            working_directory: PathBuf::from("/unused"),
            mode,
        };
        expected.extend_from_slice(&arguments);
        assert_eq!(plan.arguments(&arguments), expected);
    }
}
