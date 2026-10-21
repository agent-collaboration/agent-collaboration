//! Permanent controls for the actual fixed Normal tape builder.
use super::*;

#[test]
fn added_observations_preserve_every_original_scene_and_require_full_help_return() {
    assert_eq!(NORMAL_CHECKPOINTS.len(), 42);
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
    for (columns, rows) in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
        let label = format!("{columns}x{rows}");
        let path = output.join(format!("{label}.tape"));
        std::fs::write(&path, build_normal_component_tape(columns, rows)?)?;
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
    let script = r#"import re,sys
pattern = re.compile('(?s)' + sys.argv[1])
actual = '''│ │Options · Credits     Help│┃│
│ │東京 OpenAI  [alpha-tag]  │┃│
│ │Warning: Weekly floor    ││┃│
│ │1% blocks credit         ││┃│
│ │routing.                 ││┃│
│ │                         ││┃│
│ │Credit balance           ┃│││
│ │refreshed.               ││││'''
prefix = 'Options · Credits Help\n東京 OpenAI [alpha-tag]\n'
positives = [actual, '\n'.join(line.ljust(678) for line in actual.splitlines()),
    prefix + 'Credit balance refreshed.',
    prefix + 'Credit' + ' '*1000 + 'balance refreshed.']
negatives = [actual.replace('refreshed.', 'failed.'),
    actual.replace('Credit balance', 'balance Credit'),
    actual.replace('Credit balance', 'Credit stale balance'),
    actual.replace('refreshed.', 'refreshed.x'),
    actual.replace('refreshed.', 'unrefreshed.'),
    actual.replace('alpha-tag', 'beta-tag'),
    actual.replace('Options · Credits', 'Options · Resets'),
    actual.replace('refreshed.', 'refreshed'),
    prefix + 'Credit' + ' '*1001 + 'balance refreshed.',
    prefix + 'Credit balance' + ' '*1001 + 'refreshed.',
    prefix + 'NoCredit balance refreshed.']
assert not re.search('Credit balance refreshed', actual)
assert all(pattern.search(value) for value in positives)
assert not any(pattern.search(value) for value in negatives)
print('refresh readiness:4positive/11negative all expected')
"#;
    let result = std::process::Command::new("python3")
        .args(["-c", script, CREDIT_REFRESH_READY])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
}

#[test]
fn result_readiness_requires_complete_known_outcome_across_real_wrapped_rows() -> TestResult<()> {
    // Actual mounted32 post-repair frame, distinct from original VHS images.
    let script = r#"import re,sys
pattern = re.compile('(?s)' + sys.argv[1])
actual = '''│ │Reset result          Help│┃│
│ │東京 OpenAI  [alpha-tag]  │┃│
│ │Success — reset          ┃│┃│
│ │completed                ┃│┃│'''
prefix = 'Reset result Help\n東京 OpenAI [alpha-tag]\n'
positive = [actual, '\n'.join(line.ljust(678) for line in actual.splitlines()),
    prefix + 'Success — reset completed',
    prefix + 'Success — reset' + ' '*1000 + 'completed']
negative = [actual.replace('completed', 'comp…'),
    actual.replace('completed', ''),
    actual.replace('completed', 'failed'),
    actual.replace('completed', 'completedx'),
    actual.replace('Success — reset', 'reset — Success'),
    actual.replace('Success — reset', 'Success — failed reset'),
    actual.replace('Success — reset', 'Failure — reset'),
    actual.replace('Success — reset', 'Success reset'),
    actual.replace('alpha-tag','beta-tag'),
    actual.replace('Reset result','Options · Resets'),
    prefix + 'Success — reset' + ' '*1001 + 'completed',
    prefix + 'NotSuccess — reset completed']
assert not re.search('Success.*reset completed', actual, re.S)
assert all(pattern.search(value) for value in positive)
assert not any(pattern.search(value) for value in negative)
print('result readiness:4positive/12negative all expected')
"#;
    let result = std::process::Command::new("python3")
        .args(["-c", script, RESET_SUCCESS_READY])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
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
    let script = r#"import re, sys
pattern = re.compile(sys.argv[1])
actual = """╭──────────────────────────────╮
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
╰──────────────────────────────╯"""
valid = [actual, "❯ 東京 OpenAI Research Account Alpha Long Name"]
invalid = [
    actual.replace("❯ 東京", "  東京"),
    actual.replace("Resear", "Reseax"),
    actual.replace("ch     nt", "xx     nt"),
    actual.replace("Alpha", "Gamma"),
    actual.replace("Name", "Nope"),
    actual.replace("│ │    Resear clie", "│ │           Resear"),
    actual.replace("Resear", "ResearX"),
    actual.replace("Alpha", "Long").replace("Long   Cred", "Alpha  Cred"),
]
assert all(pattern.search(text) for text in valid), "actual wrapped selected name must match"
assert not any(pattern.search(text) for text in invalid), "wrong field/name/order/gutter must reject"
"#;
    let result = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script, &pattern])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
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
    // Text from retained original32/120 capture transcripts, never reconstructed pixels.
    let script = r#"import re, sys
phone = """╭──────────────────────────────╮
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
╰──────────────────────────────╯"""
wide = """╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
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
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯"""
claude = """╭──────────────────────────────╮
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
╰──────────────────────────────╯"""

tokyo = re.compile(sys.argv[1], re.S)
claude_pattern = re.compile(sys.argv[2], re.S)
returned = re.compile(sys.argv[3], re.S)
# Real retained text geometry plus controlled678-cell trailing whitespace,
# matching the failure mode measured in the original VHS Wait error.
valid = [phone, wide, '\n'.join(line.ljust(678) for line in phone.splitlines())]
assert all(tokyo.search(value) for value in valid), 'observed3-cell/wide/padded Tokyo rows must match'
assert claude_pattern.search(claude), 'retained selected Claude name must match'
assert claude_pattern.search('\n'.join(line.ljust(678) for line in claude.splitlines())), 'actual VHS right-padding must not block identity readiness'
invalid = [
    phone.replace('❯', ' '),
    phone.replace('Research', 'Wrong'),
    phone.replace('│  Research', '│             Research'),
    phone.replace('│  Research', '│Research'),
    phone.replace('│  Research', '│   ✕Research'),
    phone.replace('│  Account', '│  Accoun '),
    phone.replace('Alpha Long', 'Gamma Long'),
    phone.replace('Name       Credits', 'NameExtra  Credits'),
    phone.replace('Research', 'Account').replace('│  Account    ↻', '│  Research   ↻'),
]
assert not any(tokyo.search(value) for value in invalid), 'wrong name/column/gutter/prefix/fragment/suffix/order must reject'
assert not claude_pattern.search(claude.replace('ch     clie', 'zz     clie')), 'missing within-word Claude fragment rejects'
assert returned.search(phone), 'returned browse waits through actual phone name wraps'
assert not any(returned.search(value) for value in invalid), 'returned browse must retain full selected-name attribution'
print('6 positive shapes and19 negative shapes passed')
"#;
    let result = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script, &tokyo, &claude, &returned])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
}

#[test]
fn last_reset_page_wait_requires_actual_title_and_ordinal() -> TestResult<()> {
    let script = r#"import re, sys
actual = """╭──────────────────────────────╮
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
╰──────────────────────────────╯"""

pattern = re.compile(sys.argv[1], re.S)
historical = actual
# Expected text vector after Root54's literal caller-title cutover; not a new capture.
actual = historical.replace('Account options · Re', 'Options · Resets')
valid = [actual, 'Options · Resets Synthetic reset voucher 7 · available Review', '\n'.join(line.ljust(678) for line in actual.splitlines())]
invalid = [actual.replace('voucher 7', 'voucher 6'), actual.replace('voucher 7', 'voucher 70'), actual.replace('voucher 7', 'voucher 7x'), actual.replace('Synthetic reset', 'Other reset'), actual.replace('Synthetic reset', ''), actual.replace('Options · Resets', 'Credit usage'), actual.replace('Review', 'Inspect'), 'Options · Resets voucher-7 Review', historical, actual.replace('Options · Resets', 'Options · Credits')]
assert all(pattern.search(value) for value in valid), 'visible final voucher title must synchronize after bottom scroll'
assert not any(pattern.search(value) for value in invalid), 'wrong voucher/name/suffix/pane/Review must reject'
print('3positive/10negative last-page/title protocol shapes')
"#;
    let pattern = format!("(?s){LAST_RESET_VOUCHER_READY}");
    let result = std::process::Command::new("/usr/bin/python3")
        .args(["-c", script, &pattern])
        .output()?;
    if !result.status.success() {
        return Err(
            std::io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
        );
    }
    Ok(())
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
        assert_eq!(
            pace_wait.contains("Weekly[\\s│┃]{1,1000}pace[\\s│┃]{1,1000}Unknown"),
            columns == 32
        );
        assert_eq!(pace_wait.contains("burn"), columns != 32);
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
        assert_eq!(
            lower.matches("PageDown\n").count(),
            usize::from((columns, rows) == (48, 12)) * 2
        );
        assert_eq!(
            lower
                .matches("PageDown@1ms 256\nPageUp\nPageUp\nPageUp\n")
                .count(),
            usize::from((columns, rows) == (48, 12))
        );
        assert_eq!(
            lower.matches("PageUp@1ms 256\n").count(),
            usize::from((columns, rows) == (48, 12))
        );
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
