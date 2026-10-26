//! Permanent controls for the actual fixed Normal tape builder.
use super::super::vhs_wait_pattern_oracle::{ensure_wait_shapes, right_padded_rows, wait_pattern};
use super::*;

#[test]
fn added_observations_preserve_every_original_scene_and_require_full_help_return() {
    assert_eq!(NORMAL_CHECKPOINTS.len(), 43);
    for grid in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
        let tape = build_normal_component_tape(grid.0, grid.1).expect("declared viewport");
        assert!(tape.ends_with("Type \"q\"\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n"));
        let expected_help = [
            "Screenshot component-quota-confirm-yes.png",
            "Screenshot component-quota-confirm-help-top.png",
            "Screenshot component-quota-confirm-help-identity.png",
            "Screenshot component-quota-confirm-help-returned.png",
            "Screenshot component-quota-reset-result.png",
        ];
        let mut previous = 0;
        for scene in expected_help {
            let found = tape
                .find(scene)
                .expect("all Help and existing confirmation/result scenes");
            assert!(found > previous);
            previous = found;
        }
        for scene in [
            "credits-refreshed-balance",
            "credits-refreshed-freshness",
            "credits-refreshed-observation",
            "credits-policy-boundary",
        ] {
            assert!(tape.contains(&format!("Screenshot component-quota-{scene}.png")));
        }
        assert!(tape.contains("63,000"));
        assert!(tape.contains("62,500"));
        let idle_wait = tape
            .split("Screenshot component-quota-confirm-no-returned.png")
            .next()
            .unwrap()
            .lines()
            .rfind(|line| line.starts_with("Wait+Screen"))
            .unwrap();
        assert!(
            idle_wait.contains("Eligibility")
                && idle_wait.contains("Not")
                && idle_wait.contains("checked")
        );
    }
}

#[test]
#[ignore = "requires the exact nominated pinned VHS and fresh owned parser output; launches no UI"]
fn all_normal_tapes_parse_with_the_nominated_vhs_before_ui() -> TestResult<()> {
    let vhs = super::super::required_executable("QUOTA_STORYBOARD_VHS")?;
    let root = std::fs::canonicalize(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT").ok_or("fresh parser output required")?,
    )?;
    let output = root.join("normal-tape-parser-validation");
    std::fs::create_dir(&output)?;
    let normal = [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)]
        .into_iter()
        .map(|(columns, rows)| -> TestResult<(String, String)> {
            Ok((
                format!("{columns}x{rows}"),
                build_normal_component_tape(columns, rows)?,
            ))
        });
    // The hard-case tour, including its measured 48x12 poses.
    let hard_cases = [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)]
        .into_iter()
        .map(|(columns, rows)| -> TestResult<(String, String)> {
            Ok((
                format!("hard-cases-{columns}x{rows}"),
                build_hard_case_component_tape(columns, rows)?,
            ))
        });
    for entry in normal.chain(hard_cases) {
        let (label, tape) = entry?;
        let path = output.join(format!("{label}.tape"));
        std::fs::write(&path, tape)?;
        let result = std::process::Command::new(&vhs)
            .args(["validate"])
            .arg(&path)
            .output()?;
        std::fs::write(output.join(format!("{label}.stdout")), &result.stdout)?;
        std::fs::write(output.join(format!("{label}.stderr")), &result.stderr)?;
        std::fs::write(
            output.join(format!("{label}.exit")),
            result
                .status
                .code()
                .ok_or("parser exited without a code")?
                .to_string(),
        )?;
        ensure(
            result.status.success(),
            "actual VHS rejected a complete normal tape",
        )?;
    }
    Ok(())
}

#[test]
fn component_tapes_retain_layers_without_changing_declared_screens() {
    for tape in [
        build_failure_retention_control_tape(),
        build_first_frame_diagnostic_tape(),
        build_normal_component_tape(32, 24).expect("declared supported viewport"),
    ] {
        assert_eq!(tape.matches("Output diagnostic/frames/\n").count(), 1);
        assert!(tape.contains("Output screens.txt\n"));
    }
    let control = build_failure_retention_control_tape();
    assert!(control.ends_with(&format!(
        "Wait+Screen /{}/\n",
        super::super::IMPOSSIBLE_SCREEN
    )));
    assert_eq!(control.matches("Screenshot ").count(), 1);
    assert!(!control.contains("Ctrl+C\n"));
    assert!(!control.contains("Down\n"));
}

#[test]
fn normal_capture_uses_measured_positions_without_losing_id_title_or_saved_current() {
    for (columns, rows, id_down, title_down, balance_down) in [
        (32, 24, 3, 5, 0),
        (48, 12, 7, 8, 1),
        (40, 24, 3, 4, 0),
        (48, 24, 2, 3, 0),
        (76, 30, 3, 4, 0),
        (120, 36, 0, 0, 0),
    ] {
        let tape =
            build_normal_component_tape(columns, rows).expect("measured viewport is declared");
        let id_wait = "Wait+Screen /(?s)Options · Resets.*alpha-tag.*\\[voucher-7\\]/\n";
        assert!(tape.contains(&format!(
            "PageDown\nUp@1ms 256\n{}{id_wait}{}Wait+Screen /(?s){LAST_RESET_VOUCHER_READY}/\n",
            "Down\n".repeat(id_down),
            "Down\n".repeat(title_down - id_down),
        )));
        assert!(tape.contains(&format!(
                "Screenshot component-quota-credits-identity.png\nSleep 250ms\nUp@1ms 256\n{}Wait+Screen /(?s)Options · Credits.*alpha-tag.*62,500/\n",
                "Down\n".repeat(balance_down),
            )));
        assert!(tape.contains(
                "Right\nEnter\nWait+Screen /Quota status/\nCtrl+E\nUp@1ms 256\nWait+Screen /(?s)Weekly floor.*Current.*1%/\n"
            ));
    }
}

#[test]
fn refresh_readiness_accepts_wrapped_success_without_accepting_other_messages() -> TestResult<()> {
    // Text excerpt from the actual failed32 VHS screen, not reconstructed pixels.
    let pattern = wait_pattern(&format!("(?s){CREDIT_REFRESH_READY}"))?;
    let actual = "│ │Options · Credits     Help│┃│
│ │東京 OpenAI  [alpha-tag]  │┃│
│ │Warning: Weekly floor    ││┃│
│ │1% blocks credit         ││┃│
│ │routing.                 ││┃│
│ │                         ││┃│
│ │Credit balance           ┃│││
│ │refreshed.               ││││";
    let prefix = "Options · Credits Help\n東京 OpenAI [alpha-tag]\n";
    let positives = [
        actual.to_owned(),
        right_padded_rows(actual, 678),
        format!("{prefix}Credit balance refreshed."),
        format!("{prefix}Credit{}balance refreshed.", " ".repeat(1000)),
    ];
    let negatives = [
        actual.replace("refreshed.", "failed."),
        actual.replace("Credit balance", "balance Credit"),
        actual.replace("Credit balance", "Credit stale balance"),
        actual.replace("refreshed.", "refreshed.x"),
        actual.replace("refreshed.", "unrefreshed."),
        actual.replace("alpha-tag", "beta-tag"),
        actual.replace("Options · Credits", "Options · Resets"),
        actual.replace("refreshed.", "refreshed"),
        format!("{prefix}Credit{}balance refreshed.", " ".repeat(1001)),
        format!("{prefix}Credit balance{}refreshed.", " ".repeat(1001)),
        format!("{prefix}NoCredit balance refreshed."),
    ];
    ensure(
        !actual.contains("Credit balance refreshed"),
        "the retained screen wraps the message across rows",
    )?;
    ensure_wait_shapes(&pattern, &positives, &negatives)
}

#[test]
fn result_readiness_requires_complete_known_outcome_across_real_wrapped_rows() -> TestResult<()> {
    // Actual mounted32 post-repair frame, distinct from original VHS images.
    let pattern = wait_pattern(&format!("(?s){RESET_SUCCESS_READY}"))?;
    let actual = "│ │Reset result          Help│┃│
│ │東京 OpenAI  [alpha-tag]  │┃│
│ │Success — reset          ┃│┃│
│ │completed                ┃│┃│";
    let prefix = "Reset result Help\n東京 OpenAI [alpha-tag]\n";
    let positive = [
        actual.to_owned(),
        right_padded_rows(actual, 678),
        format!("{prefix}Success — reset completed"),
        format!("{prefix}Success — reset{}completed", " ".repeat(1000)),
    ];
    let negative = [
        actual.replace("completed", "comp…"),
        actual.replace("completed", ""),
        actual.replace("completed", "failed"),
        actual.replace("completed", "completedx"),
        actual.replace("Success — reset", "reset — Success"),
        actual.replace("Success — reset", "Success — failed reset"),
        actual.replace("Success — reset", "Failure — reset"),
        actual.replace("Success — reset", "Success reset"),
        actual.replace("alpha-tag", "beta-tag"),
        actual.replace("Reset result", "Options · Resets"),
        format!("{prefix}Success — reset{}completed", " ".repeat(1001)),
        format!("{prefix}NotSuccess — reset completed"),
    ];
    ensure(
        !wait_pattern("(?s)Success.*reset completed")?.is_match(actual),
        "the retained screen wraps the result across rows",
    )?;
    ensure_wait_shapes(&pattern, &positive, &negative)
}

#[test]
fn help_identity_readiness_accepts_wrapped_rows_without_other_names() -> TestResult<()> {
    // Text excerpt from the actual 76x30 Help timeout screen: the name
    // wraps onto the next modal row after background browse meter glyphs.
    let pattern = wait_pattern(&format!("(?s){}", help_identity_readiness()))?;
    let previous = wait_pattern(
        r"(?s)Quota Help.*Research[\s│┃]{1,1000}Account[\s│┃]{1,1000}Alpha[\s│┃]{1,1000}Long[\s│┃]{1,1000}Name[\s│┃]{1,1000}\[alpha-tag\]",
    )?;
    let actual = "│ │   ────│ Quota Help                                            │────┃ │ │
│ │   ████│ Account          東京 OpenAI Research Account Alpha ┃ │    ┃ │ │
│ │   ████│                  Long Name [alpha-tag]              ┃ │    ┃ │ │";
    let single = "Quota Help\nAccount 東京 OpenAI Research Account Alpha Long Name [alpha-tag]";
    let positives = [actual.to_owned(), single.to_owned()];
    let negatives = [
        actual.replace("Long Name", "Name"),
        actual.replace("Account Alpha", "Alpha"),
        actual.replace("Alpha", "Beta"),
        actual.replace("Long Name", "Name Long"),
        actual.replace("Account Alpha", "Account Gamma Alpha"),
        actual.replace("alpha-tag", "beta-tag"),
        actual.replace("Quota Help", "Quota status"),
        actual.replace("│                  Long", "                   Long"),
        single.replace("Alpha Long", "Alpha\nLong"),
    ];
    ensure(
        previous.is_match(single) && !previous.is_match(actual),
        "the previous expression accepted only the unwrapped name",
    )?;
    ensure_wait_shapes(&pattern, &positives, &negatives)
}

#[test]
fn hard_details_readiness_accepts_clipped_name_before_exact_tag() -> TestResult<()> {
    // Text excerpt from the actual hard 32x24 timeout screen.
    let pattern = wait_pattern(&format!("(?s){HARD_DETAILS_READY}"))?;
    let actual = "│ │Account details       Help│││
│ │Claude Resea… [beta-tag]  │││
│ │Status                   ┃│││
│ │Credentials locked       ││││
│ │Reason                   ││││
│ │beta detail              ││││
│ │Quota windows            ││┃│
│ │weekly                   ││┃│
│ │████ 83% left, reset 7d  ││┃│
│ │5h                       ││┃│
│ │Close                     │┃│";
    let wide = "Account details Help\nClaude Research Account Beta Full Name [beta-tag]\nStatus\nCredentials locked\nClose";
    let negatives = [
        actual.replace("Resea…", "Resea"),
        actual.replace("beta-tag", "alpha-tag"),
        actual.replace("Credentials locked", "Available"),
        actual.replace("Close", "Cancel"),
        actual.replace("Claude", "OpenAI"),
    ];
    ensure_wait_shapes(&pattern, &[actual.to_owned(), wide.to_owned()], &negatives)
}

#[test]
fn first_frame_diagnostic_finishes_before_navigation_and_cannot_satisfy_normal() {
    let tape = build_first_frame_diagnostic_tape();
    assert_eq!(
        tape.lines()
            .filter(|line| line.starts_with("Screenshot "))
            .collect::<Vec<_>>(),
        ["Screenshot component-quota-browse.png"]
    );
    assert!(tape.contains("Set Columns 48\nSet Rows 12\n"));
    assert!(tape.ends_with("Ctrl+C\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n"));
    assert!(
        !tape
            .lines()
            .any(|line| matches!(line, "Down" | "Up" | "Ctrl+R" | "Ctrl+E"))
    );
    assert!(validate_normal_checkpoints(&["component-quota-browse"]).is_err());
    assert!(validate_normal_checkpoints(&NORMAL_CHECKPOINTS).is_ok());
}

#[test]
fn declared_tours_use_supported_body_navigation_and_complete_scene_order() {
    for (columns, rows) in [(120, 36), (76, 30), (48, 24), (48, 12), (40, 24), (32, 24)] {
        let tape = build_normal_component_tape(columns, rows).expect("declared tour viewport");
        let scenes = tape
            .lines()
            .filter_map(|line| {
                line.strip_prefix("Screenshot ")
                    .and_then(|path| path.strip_suffix(".png"))
            })
            .collect::<Vec<_>>();
        assert_eq!(scenes, NORMAL_CHECKPOINTS);
        // The pinned VHS sends Pages, not Home/End. After the bounded browse
        // return, dialogs retain their established arrow-key journey.
        let (_, dialogs) = tape
            .split_once("Screenshot component-quota-browse-page-returned.png\n")
            .expect("browse must return before the dialog tour");
        assert!(!dialogs.lines().any(|line| line == "Home" || line == "End"));
        assert!(!tape.lines().any(|line| matches!(
            line,
            "Home" | "End" | "PgUp" | "PgDown" | "Ctrl+Home" | "Ctrl+End"
        )));
        assert!(tape.contains("Up@1ms 256\n"));
        assert!(tape.contains("Down@1ms 256\n"));
        assert!(tape.contains("Type \"zznomatchzz\"\n"));
    }
}

#[test]
fn selected_account_wait_matches_actual_phone_fragments_only() -> TestResult<()> {
    let pattern = format!(
        "(?s){}",
        selected_account_readiness_expression(&[
            "東京", "OpenAI", "Research", "Account", "Alpha", "Long", "Name",
        ])
    );
    // Retained current-component32x24 paint from c7-phone-normal-fixture-preflight.log.
    // This checks the generated common regex subset, not fabricated pixels.
    let pattern = wait_pattern(&pattern)?;
    let actual = "╭──────────────────────────────╮
│ Quota status    Details Help │
│ Pool-wide: responses         │
│ Accounts 2 · Usable 2        │
│ All · Disabled: Shown · 2/2 ▾│
│ ┌──────────────────────────┐ │
│ │    Accoun Stat Quota     │ │
│ │    t      us             │ │
│ │  ❯ 東京   ✓    alpha   ┃ │ │
│ │    OpenAI 1    detail  ┃ │ │
│ │    Resear clie Weekly  ┃ │ │
│ │    ch     nt   █████   ┃ │ │
│ │    Accoun ↻ 2  83%     │ │ │
│ │    t      rese left,   │ │ │
│ │    Alpha  ts   reset   │ │ │
│ │    Long   Cred 7d      │ │ │
│ │    Name   its  5h      │ │ │
│ │    ◎      62.5 █████   │ │ │
│ │    OpenAI k    99%     │ │ │
│ │           fres left,   │ │ │
│ │           h    reset   │ │ │
│ │                5h      │ │ │
│ └──────────────────────────┘ │
╰──────────────────────────────╯";
    // Actual wrapped selected name must match.
    let valid = [
        actual.to_owned(),
        "❯ 東京 OpenAI Research Account Alpha Long Name".to_owned(),
    ];
    // Wrong field/name/order/gutter must reject.
    let invalid = [
        actual.replace("❯ 東京", "  東京"),
        actual.replace("Resear", "Reseax"),
        actual.replace("ch     nt", "xx     nt"),
        actual.replace("Alpha", "Gamma"),
        actual.replace("Name", "Nope"),
        actual.replace("│ │    Resear clie", "│ │           Resear"),
        actual.replace("Resear", "ResearX"),
        actual
            .replace("Alpha", "Long")
            .replace("Long   Cred", "Alpha  Cred"),
    ];
    ensure_wait_shapes(&pattern, &valid, &invalid)
}

#[test]
fn selected_account_wait_accepts_observed_phone_prefix_and_padded_vhs_rows() -> TestResult<()> {
    let tokyo = selected_account_readiness_expression(&[
        "東京", "OpenAI", "Research", "Account", "Alpha", "Long", "Name",
    ]);
    let claude = selected_account_readiness_expression(&[
        "Claude", "Research", "Account", "Beta", "Full", "Name",
    ]);
    let tape = build_normal_component_tape(32, 24)?;
    let returned = tape
        .lines()
        .rev()
        .find_map(|line| {
            line.strip_prefix("Wait+Screen /(?s)Quota status")
                .and_then(|tail| tail.strip_suffix('/'))
        })
        .expect("returned browse readiness must remain declared");
    let returned = format!("(?s)Quota status{returned}");
    let tokyo = wait_pattern(&format!("(?s){tokyo}"))?;
    let claude = wait_pattern(&format!("(?s){claude}"))?;
    let returned = wait_pattern(&returned)?;
    // Text from retained original32/120 capture transcripts, never reconstructed pixels.
    let phone = "╭──────────────────────────────╮
│Quota status      Details Help│
│Pool-wide: responses          │
│Accounts 2 · Usable 2         │
│All · Disabled: Shown · 2/2 ▾ │
│  Account    Status  Quota    │
│❯ 東京       ✓       alpha   ┃│
│  OpenAI     1       detail  ┃│
│  Research   client  Weekly  ┃│
│  Account    ↻ 2     █████   ┃│
│  Alpha Long resets  83%     ┃│
│  Name       Credits left,   ┃│
│  ◎ OpenAI   62.5k   reset 7d┃│
│             fresh   5h      ││
│                     █████   ││
│                     99%     ││
│                     left,   ││
│                     reset 5h││
│                     Weekly  ││
│                     pace    ││
│                     □□□□□□□│││
│                     □□□□□□□ ││
│                     burn    ││
╰──────────────────────────────╯";
    let wide = "╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ Quota status                                           Clients 1 · Fresh 2 · Stale 0 · Data unknown 0  Details  Help │
│ Pool-wide: responses  Accounts 2 · Usable 2 · Reserve 0 · Blocked 0 · Pool unknown 0 · Excluded 0                    │
│ Filters ▾  Provider: All · Disabled: Shown · 2/2 accounts                                                            │
│ ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │    Account                                    Status                        Quota                                │ │
│ │  ❯ 東京 OpenAI Research Account Alpha Long    ✓ available                   alpha detail                         │ │
│ │    Name                                       1 client                      Weekly       █████ 83% left, reset   │ │
│ │    ◎ OpenAI                                   ↻ 2 resets                                 7d                      │ │
│ │                                               Credits 62.5k fresh           5h           █████ 99% left, reset   │ │
│ │                                                                                          5h                      │ │
│ │                                                                             Weekly pace  □□□□□□□│□□□□□□□  burn   │ │
│ │                                                                                          unavailable             │ │
│ │                                                                                                                  │ │
│ │    Claude Research Account Beta Full Name     ✓ available                   beta detail                          │ │
│ │    ◆ Claude                                   0 clients                     Weekly       ████ 75% left, reset    │ │
│ │                                               ↻ 2 resets                                 6d                      │ │
│ │                                               Credits unknown               5h           ████ 70% left, reset    │ │
│ │                                                                                          4h                      │ │
│ │                                                                             Weekly pace  □□□□□□□│□□□□□□□  burn   │ │
│ │                                                                                          unavailable             │ │
│ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯";
    let claude_screen = "╭──────────────────────────────╮
│ Quota status    Details Help │
│ Pool-wide: responses         │
│ Accounts 2 · Usable 2        │
│ All · Disabled: Shown · 2/2 ▾│
│ ┌──────────────────────────┐ │
│ │    Accoun Stat Quota     │ │
│ │    t      us             │ │
│ │  ❯ Claude ✓    beta    │ │ │
│ │    Resear 0    detail  │ │ │
│ │    ch     clie Weekly  │ │ │
│ │    Accoun nts  ████    │ │ │
│ │    t Beta ↻ 2  75%     │ │ │
│ │    Full   rese left,   │ │ │
│ │    Name   ts   reset   │ │ │
│ │    ◆      Cred 6d      ┃ │ │
│ │    Claude its  5h      ┃ │ │
│ │           unkn ████    ┃ │ │
│ │           own  70%     ┃ │ │
│ │                left,   │ │ │
│ │                reset   │ │ │
│ │                4h      │ │ │
│ └──────────────────────────┘ │
╰──────────────────────────────╯";
    // Real retained text geometry plus controlled678-cell trailing whitespace,
    // matching the failure mode measured in the original VHS Wait error.
    // Observed3-cell/wide/padded Tokyo rows must match; wrong
    // name/column/gutter/prefix/fragment/suffix/order must reject.
    let invalid = [
        phone.replace('❯', " "),
        phone.replace("Research", "Wrong"),
        phone.replace("│  Research", "│             Research"),
        phone.replace("│  Research", "│Research"),
        phone.replace("│  Research", "│   ✕Research"),
        phone.replace("│  Account", "│  Accoun "),
        phone.replace("Alpha Long", "Gamma Long"),
        phone.replace("Name       Credits", "NameExtra  Credits"),
        phone
            .replace("Research", "Account")
            .replace("│  Account    ↻", "│  Research   ↻"),
    ];
    ensure_wait_shapes(
        &tokyo,
        &[
            phone.to_owned(),
            wide.to_owned(),
            right_padded_rows(phone, 678),
        ],
        &invalid,
    )?;
    // Retained selected Claude name must match, and actual VHS right-padding
    // must not block identity readiness; a missing within-word fragment rejects.
    ensure_wait_shapes(
        &claude,
        &[
            claude_screen.to_owned(),
            right_padded_rows(claude_screen, 678),
        ],
        &[claude_screen.replace("ch     clie", "zz     clie")],
    )?;
    // Returned browse waits through actual phone name wraps and retains full
    // selected-name attribution.
    ensure_wait_shapes(&returned, &[phone.to_owned()], &invalid)
}

#[test]
fn last_reset_page_wait_requires_actual_title_and_ordinal() -> TestResult<()> {
    let pattern = wait_pattern(&format!("(?s){LAST_RESET_VOUCHER_READY}"))?;
    let historical = "╭──────────────────────────────╮
│Quota status      Details Help│
│Pool-wide: responses          │
│Accounts 2 · Usable 2         │
│  Account    Status  Quota    │
│❯ 東京       ✓       alpha   ┃│
│ ╭──────────────────────────╮┃│
│ │Account options · Re  Help│┃│
│ │東京 OpenAI  [alpha-tag]  │┃│
│ │Synthetic reset          ││┃│
│ │voucher 7 · available    ││┃│
│ │· does not expire        ││┃│
│ │Saved count differs:     ││┃│
│ │2 resets saved, 7        ││││
│ │live.                    ┃│││
│ │                         ┃│││
│ │                          │││
│ │Cancel  Review            │││
│ ╰──────────────────────────╯││
│                     □□□□□□□│││
│                     □□□□□□□ ││
│                     burn    ││
│                     unavaila││
╰──────────────────────────────╯";
    // Expected literal caller-title text vector; this is a control, not a capture.
    let actual = historical.replace("Account options · Re", "Options · Resets");
    // Visible final voucher title must synchronize after bottom scroll.
    let valid = [
        actual.clone(),
        "Options · Resets Synthetic reset voucher 7 · available Review".to_owned(),
        right_padded_rows(&actual, 678),
    ];
    // Wrong voucher/name/suffix/pane/Review must reject.
    let invalid = [
        actual.replace("voucher 7", "voucher 6"),
        actual.replace("voucher 7", "voucher 70"),
        actual.replace("voucher 7", "voucher 7x"),
        actual.replace("Synthetic reset", "Other reset"),
        actual.replace("Synthetic reset", ""),
        actual.replace("Options · Resets", "Credit usage"),
        actual.replace("Review", "Inspect"),
        "Options · Resets voucher-7 Review".to_owned(),
        historical.to_owned(),
        actual.replace("Options · Resets", "Options · Credits"),
    ];
    ensure_wait_shapes(&pattern, &valid, &invalid)
}

#[test]
fn component_contract_rejects_missing_cancel_or_saved_state() {
    assert!(validate_normal_checkpoints(&NORMAL_CHECKPOINTS).is_ok());
    let mut missing = NORMAL_CHECKPOINTS.to_vec();
    missing.retain(|scene| *scene != "component-quota-policy-cancelled");
    assert!(validate_normal_checkpoints(&missing).is_err());
    let mut wrong = NORMAL_CHECKPOINTS.to_vec();
    let saved = wrong
        .iter()
        .position(|scene| *scene == "component-quota-policy-saved")
        .unwrap();
    wrong[saved] = "component-quota-policy-draft";
    assert!(validate_normal_checkpoints(&wrong).is_err());
    let mut reordered = NORMAL_CHECKPOINTS.to_vec();
    reordered.swap(20, 22);
    assert!(validate_normal_checkpoints(&reordered).is_err());
}

#[test]
fn lower_browse_evidence_keeps_all_facts_and_restores_identity_before_switching_account() {
    for (columns, rows) in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
        let tape = build_normal_component_tape(columns, rows).expect("declared real fixture grid");
        let pace_wait = tape
            .split("Screenshot component-quota-browse-pace-value.png")
            .next()
            .unwrap()
            .lines()
            .rfind(|line| line.starts_with("Wait+Screen"))
            .unwrap();
        // The pace value is "burn unavailable" at every grid (C1, 2026-10-23).
        assert!(!pace_wait.contains("Unknown"));
        assert!(pace_wait.contains("burn"));
        let credit_wait = tape
            .split("Screenshot component-quota-browse-credit-window.png")
            .next()
            .unwrap()
            .lines()
            .rfind(|line| line.starts_with("Wait+Screen"))
            .unwrap();
        assert!(credit_wait.contains(r"62\.5k credits"));
        let mut previous = 0;
        for checkpoint in [
            "browse",
            "browse-credit-window",
            "browse-pace-heading",
            "browse-pace-value",
            "browse-page-returned",
            "next-account",
            "restored-account",
        ] {
            let position = tape
                .find(&format!("Screenshot component-quota-{checkpoint}.png"))
                .expect("every identity and lower-content original remains declared");
            assert!(position > previous);
            previous = position;
        }
        let lower = tape
            .split("Screenshot component-quota-browse.png")
            .nth(1)
            .unwrap()
            .split("Screenshot component-quota-browse-page-returned.png")
            .next()
            .unwrap();
        // The short viewport uses the independently witnessed complete groups,
        // with an acknowledged bottom between forward and reverse paging.
        if (columns, rows) == (48, 12) {
            let bottom = "PageDown@1ms 256\nWait+Screen /(?s)Quota status.*Extra.*Credits unknown.*2 resets/\n";
            assert_eq!(lower.matches(bottom).count(), 2);
            assert!(lower.contains(&format!(
                "{bottom}{}Wait+Screen /(?s)Quota status.*62\\.5k credits/",
                "PageUp\n".repeat(5)
            )));
            let value_pose = format!("{bottom}{}", "PageUp\n".repeat(6));
            assert!(lower.contains(&value_pose));
            assert_eq!(lower.matches("PageDown\n").count(), 2, "heading pose only");
            assert_eq!(lower.matches("PageUp@1ms 256\n").count(), 3);
        } else {
            // Other grids retain their existing forward-only measured poses.
            let pages: [usize; 3] = match (columns, rows) {
                (48, 24) | (32, 24) => [1, 0, 0],
                _ => [0, 0, 0],
            };
            assert_eq!(
                lower.matches("PageDown\n").count(),
                pages.iter().sum::<usize>()
            );
            assert!(!lower.contains("PageDown@1ms 256"));
            assert_eq!(
                lower.matches("PageUp@1ms 256\n").count(),
                pages.iter().filter(|count| **count > 0).count()
            );
            assert!(lower.contains(&format!(
                "{}Wait+Screen /(?s)Quota status.*62\\.5k credits/",
                "PageDown\n".repeat(pages[0])
            )));
        }
        let help = tape
            .split("Screenshot component-quota-help-top.png")
            .nth(1)
            .unwrap()
            .split("Screenshot component-quota-help-bottom.png")
            .next()
            .unwrap();
        assert_eq!(
            help.matches("Up\n").count(),
            usize::from((columns, rows) == (48, 12))
        );
        let entry = tape
            .split("Escape\nCtrl+R\n")
            .nth(1)
            .unwrap()
            .split("Screenshot component-quota-options-identity.png")
            .next()
            .unwrap();
        assert!(
            !entry.contains("Up@") && !entry.contains("Down@"),
            "initial means before manual scroll"
        );
    }
}

#[test]
fn hard_case_tour_declares_every_grid_and_exact_rendered_facts() {
    for (columns, rows) in [(120, 36), (76, 30), (48, 24), (48, 12), (40, 24), (32, 24)] {
        let tape = build_hard_case_component_tape(columns, rows).expect("declared grid");
        let shots: Vec<&str> = tape
            .lines()
            .filter_map(|line| line.strip_prefix("Screenshot "))
            .map(|name| name.trim_end_matches(".png"))
            .collect();
        let expected: &[&str] = match (columns, rows) {
            (48, 12) => &HARD_CASE_SHORT_CHECKPOINTS,
            (120, 36) => &HARD_CASE_CHECKPOINTS,
            _ => &HARD_CASE_PARTIAL_CHECKPOINTS,
        };
        assert_eq!(shots, expected);
        validate_hard_case_checkpoints(columns, rows, &shots)
            .expect("the consumer accepts the tour");
        // Literals come from the fixture through render_window/extra_group.
        for fact in [
            "Credentials locked",
            "92% · week · 6d 16h",
            "83% · 5h · 2h 10m",
            "75% · week · —",
            "week stale",
            "No data · 5h · —",
            r"125\.5 credits",
            "Credits stale",
            "Pool details.*Accounts: 2.*Usable: 1.*Reserve: 0.*Close",
            "Pool details.*Blocked: 1.*Pool unknown: 0.*Excluded: 0.*Fresh: 0.*Close",
            "Pool details.*Stale: 1.*Data unknown: 1.*Clients: 1.*Credentials locked.*Close",
        ] {
            assert!(tape.contains(fact), "{fact}");
        }
        // The whole census body is one pose only where it fits (120x36).
        assert_eq!(
            tape.contains("Accounts: 2.*Usable: 1.*Reserve: 0.*Blocked: 1.*Pool unknown: 0.*Excluded: 0.*Fresh: 0.*Stale: 1.*Data unknown: 1.*Clients: 1"),
            (columns, rows) == (120, 36)
        );
        assert!(tape.contains("Down\n"), "Beta is selected before its facts");
        assert!(
            tape.contains("F5\nUp@1ms 256\n"),
            "Pool details opens with F5"
        );
        assert!(tape.contains("Escape\n"), "Pool details closes with Esc");
        assert!(tape.ends_with("Type \"q\"\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n"));
    }
    // 48x12 measured page counts: name continuation 1,
    // week 2, 5h 3, back to the top and Down to Beta, Beta continuation 1, Beta
    // week 2, stale/no data 3, credits 6 (totals unchanged by the continuations).
    let short = build_hard_case_component_tape(48, 12).expect("measured short grid");
    for step in [
        "Screenshot component-quota-hard-browse-top.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-alpha-name-continued.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-alpha-week.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-alpha-short.png\nSleep 250ms\nPageUp\nPageUp\nPageUp\nDown\n",
        "Screenshot component-quota-hard-beta-selected.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-beta-name-continued.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-beta-week.png\nSleep 250ms\nPageDown\n",
        "Screenshot component-quota-hard-beta-short.png\nSleep 250ms\nPageDown\nPageDown\nPageDown\n",
        "Screenshot component-quota-hard-beta-extra.png\nSleep 250ms\nPageUp\nPageUp\nPageUp\nPageUp\nPageUp\nPageUp\nF5\n",
    ] {
        assert!(short.contains(step), "{step}");
    }
    assert!(!short.contains("component-quota-hard-pool-details-complete"));
    assert!(build_hard_case_component_tape(48, 13).is_err());
    let mut reordered = HARD_CASE_CHECKPOINTS.to_vec();
    reordered.swap(1, 2);
    assert!(validate_hard_case_checkpoints(120, 36, &reordered).is_err());
    assert!(validate_hard_case_checkpoints(120, 36, &HARD_CASE_CHECKPOINTS[1..]).is_err());
    assert!(validate_hard_case_checkpoints(48, 12, &HARD_CASE_CHECKPOINTS).is_err());
}

#[test]
fn hard_beta_windows_waits_bind_the_wrapped_selection_and_facts_after_its_marker() -> TestResult<()>
{
    // Actual hard-case 120x36 VHS screen text, not pixels: Beta's name wraps
    // after "Account" with its week window on the marker row.
    let actual = "│ Quota status                                                                           Clients 1  Pool details  Help │
│ Credentials locked                                                                                                   │
│ Filters ▾  Provider: All · Disabled: Shown · 2/2 accounts                                                            │
│ ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │   Account                 Quota left · reset in                 Weekly pace              Extra                   │ │
│ │   ─────────────────────── ───────────────────────────────────── ──────────────────────── ──────────────────────  │ │
│ │   東京 OpenAI Research    ██████░ 92% · week · 6d 16h           burn unavailable         62.5k credits           │ │
│ │   Account Alpha Long Name █████░░ 83% · 5h · 2h 10m             reset pace unavailable   2 resets                │ │
│ │   ◎ OpenAI · 1 client                                                                                            │ │
│ │   Credentials locked                                                                                             │ │
│ │                                                                                                                  │ │
│ │                                                                                                                  │ │
│ │ ❯ Claude Research Account █████░░ 75% · week · —                burn unavailable         125.5 credits           │ │
│ │   Beta Full Name          week stale                            reset pace unavailable   Credits stale           │ │
│ │   ◆ Claude · 0 clients    No data · 5h · —                                               2 resets                │ │
│ │   Credentials locked                                                                                             │ │";
    let name = wait_pattern(&format!("(?s){}", hard_beta_selected_name_readiness()))?;
    let facts = wait_pattern(&format!("(?s){HARD_BETA_WINDOW_FACTS_READY}"))?;
    let previous = wait_pattern(&format!(
        "(?s){}.*75% · week · —.*week stale.*No data · 5h · —",
        hard_beta_selected_name_readiness()
    ))?;
    ensure(
        !previous.is_match(actual),
        "the single post-name expression rejected this retained wrapped frame",
    )?;
    let screens = [actual.to_owned(), right_padded_rows(actual, 2400)];
    // The marker must start Beta's whole wrapped name.
    ensure_wait_shapes(
        &name,
        &screens,
        &[
            actual
                .replace("│ │ ❯ Claude", "│ │   Claude")
                .replace("│ │   東京", "│ │ ❯ 東京"),
            actual.replace(
                "Beta Full Name          week",
                "Beta Full               week",
            ),
            actual.replace("Beta Full Name", "Gamma Full Name"),
            actual.replace("Claude Research Account █", "Claude Research          █"),
        ],
    )?;
    // Every Beta window fact must follow the selection marker.
    ensure_wait_shapes(
        &facts,
        &screens,
        &[
            actual.replace("75% · week · —", "75% · week · 1d"),
            actual.replace("week stale", "week fresh"),
            actual.replace("No data · 5h · —", "No data · 5h · 1h"),
            actual.replace('❯', " "),
            actual
                .replace("█████░░ 75% · week · —", "█████░░ 80% · week · —")
                .replace("92% · week · 6d 16h", "75% · week · —"),
        ],
    )
}
