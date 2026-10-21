//! VHS inputs and literal compact scene contract; owned lifecycle stays separate.
use super::{CapturedTuiSurface, FONT_FAMILY, TestResult, ensure};
#[path = "provider_filter_details.rs"]
mod provider_filter_details;
use provider_filter_details::{compact_session_details_poses, expected_session_checkpoints};

pub(super) fn validate_compact_checkpoints(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
    checkpoints: &[&str],
) -> TestResult<()> {
    let complete = match surface {
        CapturedTuiSurface::Sessions => {
            checkpoints == expected_session_checkpoints(columns, rows).as_slice()
        }
        CapturedTuiSurface::Quota => {
            checkpoints
                == [
                    "compact-quota-all",
                    "compact-quota-provider-no-match",
                    "compact-quota-chooser",
                    "compact-quota-help",
                    "compact-quota-disabled-choice",
                    "compact-quota-hidden-choice",
                    "compact-quota-claude",
                    "compact-quota-cancelled",
                    "compact-quota-details",
                ]
        }
    };
    ensure(
        complete,
        "compact capture must contain every declared menu checkpoint",
    )
}

pub(super) fn build_filter_tape(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
    empty: bool,
) -> (String, Vec<&'static str>) {
    let mut tape = format!(
        "Set Shell bash\nSet Columns {columns}\nSet Rows {rows}\nSet FontFamily {FONT_FAMILY}\nSet FontSize 16\nSet Padding 0\nSet CursorBlink false\nSet WaitTimeout 8s\nOutput screens.txt\n"
    );
    let mut checkpoints = Vec::new();
    match surface {
        CapturedTuiSurface::Sessions => {
            checkpoint(&mut tape, &mut checkpoints, "sessions-all", "6/6");
            tape.push_str("Ctrl+F\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-chooser",
                "Draft: All providers",
            );
            tape.push_str("Down\nEnter\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-codex", "2/6.*Codex");
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-details",
                "Session details.*Codex Alpha",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-details-closed",
                "2/6.*Codex",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-codex-bottom",
                "Codex Beta",
            );
            tape.push_str("Ctrl+F\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-claude",
                "2/6.*Claude Code",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-claude-bottom",
                "Claude Delta",
            );
            tape.push_str("Ctrl+F\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-cursor",
                "1/6.*Cursor",
            );
            tape.push_str("Ctrl+F\nUp\nUp\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-draft",
                "Draft: Codex",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-cancelled",
                "1/6.*Cursor",
            );
            tape.push_str("Up\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-new-choice",
                "New Codex session",
            );
            tape.push_str("Type \"zznomatchzz\"\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-empty", "0/6");
            tape.push_str("Escape\nCtrl+F\nUp\nUp\nUp\nEnter\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-restored", "6/6");
            // Complementary final page demonstrates rows below the first viewport.
            tape.push_str("Down\nDown\nDown\nDown\nDown\nDown\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-all-bottom",
                "Claude-lookalike Zeta",
            );
            tape.push_str("Ctrl+D\n");
        }
        CapturedTuiSurface::Quota => {
            if empty {
                tape.push_str(
                    "Wait+Screen /4\\/4 accounts/\nCtrl+F\nDown\nDown\nTab\nSpace\nEnter\n",
                );
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-empty",
                    "No accounts match these filters",
                );
            } else {
                checkpoint(&mut tape, &mut checkpoints, "quota-all", "4/4 accounts");
                tape.push_str("Up\nUp\nUp\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-all-top", "Claude Delta");
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-all-next",
                    "Claude Gamma",
                );
                tape.push_str("Ctrl+F\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-chooser", "Draft: All");
                tape.push_str("Down\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-openai",
                    "OpenAI.*2/4 accounts",
                );
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-openai-bottom",
                    "OpenAI Beta",
                );
                tape.push_str("Ctrl+F\nDown\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-claude",
                    "Claude.*2/4 accounts",
                );
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-claude-bottom",
                    "Claude Gamma",
                );
                tape.push_str("Ctrl+F\nTab\nSpace\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-hidden",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("F4\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-details",
                    "Account details",
                );
                tape.push_str("Escape\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-details-closed",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("Ctrl+F\nUp\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-draft", "Draft: OpenAI");
                tape.push_str("Escape\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-cancelled",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("Ctrl+F\nUp\nUp\nTab\nSpace\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-restored",
                    "All.*Disabled: Shown.*4/4 accounts",
                );
                tape.push_str("Down\nDown\nDown\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-all-bottom",
                    "OpenAI Beta",
                );
            }
            tape.push_str("Type \"q\"\n");
        }
    }
    tape.push_str("Wait+Screen /QUOTA_STORYBOARD_CHILD_EXIT=0/\n");
    (tape, checkpoints)
}

fn checkpoint(
    tape: &mut String,
    checkpoints: &mut Vec<&'static str>,
    name: &'static str,
    expected: &str,
) {
    let expression = expected.replace('/', "\\/");
    tape.push_str(&format!(
        "Wait+Screen /(?s){expression}/\nSleep 250ms\nScreenshot {name}.png\nSleep 250ms\n"
    ));
    checkpoints.push(name);
}

pub(super) fn build_compact_filter_tape(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
) -> (String, Vec<&'static str>) {
    let mut tape = format!(
        "Set Shell bash\nSet Columns {columns}\nSet Rows {rows}\nSet FontFamily {FONT_FAMILY}\nSet FontSize 16\nSet Padding 0\nSet CursorBlink false\nSet WaitTimeout 8s\nOutput screens.txt\n"
    );
    let mut checkpoints = Vec::new();
    match surface {
        CapturedTuiSurface::Sessions => {
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-all",
                "6/6.*Codex Alpha",
            );
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-codex-details",
                "Session details.*Codex Alpha",
            );
            // Details starts on Resume; backwards focus reaches Fork. Forward
            // focus wraps to Close and would never reach this next scene.
            tape.push_str("Shift+Tab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-fork",
                "Fork session.*Cancel",
            );
            tape.push_str("Tab\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-fork-choice",
                "Fork session.*Destination[:]?.*Cancel.*Select",
            );
            tape.push_str("Escape\nEscape\nWait+Screen /(?s)6\\/6.*Codex Alpha/\nCtrl+N\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new",
                "New session.*Cancel.*Start",
            );
            tape.push_str("Shift+Tab\nShift+Tab\nShift+Tab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-machine",
                "New session.*Machine[:]?.*Cancel.*Select",
            );
            tape.push_str("Type \"zznomatchzz\"\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-no-match",
                r"New session.*Machine[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz.*No matching[ \t\r\n│┃┌┐]{1,128}choices.*Cancel.*Select",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-returned",
                "New session.*Cancel.*Start",
            );
            tape.push_str("Escape\nWait+Screen /(?s)6\\/6.*Codex Alpha/\nCtrl+G\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-machine-sources",
                "Choose machine.*Machine[:]?.*Cancel.*Select",
            );
            tape.push_str("Escape\nCtrl+F\nEnter\nType \"zznomatchzz\"\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-client-no-match",
                r"Session filters.*Client[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz.*No matching[ \t\r\n│┃┌┐]{1,128}choices.*Cancel.*Select",
            );
            tape.push_str("Escape\nEnter\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-chooser",
                r"Session filters.*Client[ \t\r\n│┃┌┐]{1,128}Codex.*Cancel.*Apply",
            );
            tape.push_str("F1\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-help",
                "Sessions Help.*Move field focus",
            );
            tape.push_str("Escape\nWait+Screen /(?s)Session filters.*Client.*Codex.*Cancel.*Apply/\nEnter\nDown\nEnter\nTab\nTab\nTab\nTab\nTab\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-claude",
                "2/6.*Claude Gamma",
            );
            tape.push_str("Ctrl+F\nEnter\nDown\nEnter\nWait+Screen /(?s)Session filters.*Client.*Cursor.*Cancel.*Apply/\nEscape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-cancelled",
                "2/6.*Claude Gamma",
            );
            tape.push_str("F4\n");
            let mut previous = 0;
            for &(name, down) in compact_session_details_poses(columns, rows) {
                for _ in previous..down {
                    tape.push_str("Down\n");
                }
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    name,
                    "Session details.*Claude Gamma",
                );
                previous = down;
            }
            tape.push_str("Escape\nWait+Screen /(?s)2\\/6.*Claude Gamma/\nCtrl+F\nTab\nEnter\nType \"Checkout\"\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-folder-choice",
                "Session filters.*Folder[:]?.*Checkout.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-model-provider-choice",
                "Session filters.*Model provider[:]?.*Any model provider.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-source-choice",
                "Session filters.*Source[:]?.*All sources.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-runtime-choice",
                "Session filters.*Runtime[:]?.*Active.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-sort-choice",
                "Session filters.*Sort[:]?.*Updated.*Cancel.*Select",
            );
            tape.push_str("Escape\nEscape\nWait+Screen /(?s)2\\/6.*Claude Gamma/\nCtrl+D\n");
        }
        CapturedTuiSurface::Quota => {
            tape.push_str(
                "Wait+Screen /(?s)All.*Disabled: Shown.*4\\/4( accounts| ▾)/\nUp\nUp\nUp\n",
            );
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-all",
                "All.*Disabled: Shown.*4/4( accounts| ▾).*Claude Delta",
            );
            tape.push_str("Ctrl+F\nEnter\nType \"zznomatchzz\"\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-provider-no-match",
                r"Account filters.*Account provider[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz.*No matching[ \t\r\n│┃┌┐]{1,128}choices",
            );
            tape.push_str("Escape\nEnter\nDown\nEnter\nDown\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-chooser",
                r"Account filters.*Account provider[ \t]+OpenAI.*Disabled accounts[ \t]+Shown",
            );
            tape.push_str("Up\nF1\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-help",
                "Quota Help.*Move field",
            );
            tape.push_str("Escape\nWait+Screen /(?s)Account filters.*Account provider[ \\t]+OpenAI/\nEnter\nDown\nEnter\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-disabled-choice",
                "Account filters.*Disabled accounts[:]?.*[›❯>] Shown",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-hidden-choice",
                "Account filters.*Shown.*[›❯>] Hidden",
            );
            tape.push_str("Enter\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-claude",
                "Claude.*Disabled: Hidden.*1/4( accounts| ▾).*Claude Gamma",
            );
            tape.push_str("Ctrl+F\nEnter\nUp\nEnter\nWait+Screen /(?s)Account filters.*Account provider[ \\t]+OpenAI/\nEscape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-cancelled",
                "Claude.*Disabled: Hidden.*1/4( accounts| ▾).*Claude Gamma",
            );
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-details",
                "Account details.*Claude Gamma",
            );
            tape.push_str("Escape\nWait+Screen /(?s)Claude.*Disabled: Hidden.*1\\/4( accounts| ▾).*Claude Gamma/\nType \"q\"\n");
        }
    }
    tape.push_str("Wait+Screen /QUOTA_STORYBOARD_CHILD_EXIT=0/\n");
    (tape, checkpoints)
}

#[cfg(test)]
mod compact_capture_contract_tests {
    use super::*;

    #[test]
    #[ignore = "requires the exact nominated pinned VHS and fresh parser output; launches no UI"]
    fn all_compact_tapes_parse_with_the_nominated_vhs_before_ui() -> TestResult<()> {
        let vhs = super::super::required_executable("QUOTA_STORYBOARD_VHS")?;
        let root = std::fs::canonicalize(
            std::env::var_os("QUOTA_STORYBOARD_OUTPUT").ok_or("fresh parser output required")?,
        )?;
        let output = root.join("compact-tape-parser-validation");
        std::fs::create_dir(&output)?;
        for surface in [CapturedTuiSurface::Sessions, CapturedTuiSurface::Quota] {
            for (columns, rows) in crate::CURRENT_STATIC_VIEWPORTS {
                let label = format!("{}-{columns}x{rows}", surface.label());
                let (tape, checkpoints) = build_compact_filter_tape(surface, columns, rows);
                validate_compact_checkpoints(surface, columns, rows, &checkpoints)?;
                let path = output.join(format!("{label}.tape"));
                std::fs::write(&path, tape)?;
                let result = std::process::Command::new(&vhs)
                    .arg("validate")
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
                    "actual VHS rejected a complete compact tape",
                )?;
            }
        }
        Ok(())
    }

    #[test]
    fn compact_quota_consumer_accepts_every_required_named_scene() -> TestResult<()> {
        for (columns, rows) in [(120, 36), (76, 30), (48, 24), (48, 12)] {
            let (_, checkpoints) =
                build_compact_filter_tape(CapturedTuiSurface::Quota, columns, rows);
            validate_compact_checkpoints(CapturedTuiSurface::Quota, columns, rows, &checkpoints)?;
        }
        Ok(())
    }

    #[test]
    fn compact_quota_consumer_rejects_missing_or_replaced_named_scenes() {
        let (_, checkpoints) = build_compact_filter_tape(CapturedTuiSurface::Quota, 120, 36);
        for omitted_index in 0..checkpoints.len() {
            let mut missing = checkpoints.clone();
            missing.remove(omitted_index);
            assert!(
                validate_compact_checkpoints(CapturedTuiSurface::Quota, 120, 36, &missing).is_err(),
                "missing scene {} must reject before capture",
                checkpoints[omitted_index]
            );
        }
        let mut replaced = checkpoints.clone();
        replaced[7] = replaced[0];
        assert!(
            validate_compact_checkpoints(CapturedTuiSurface::Quota, 120, 36, &replaced).is_err()
        );
        let mut extra = checkpoints;
        extra.push("unexpected-scene");
        assert!(validate_compact_checkpoints(CapturedTuiSurface::Quota, 120, 36, &extra).is_err());
    }

    #[test]
    fn compact_capture_preserves_each_declared_menu_checkpoint() {
        for (surface, expected) in [
            (
                CapturedTuiSurface::Sessions,
                [
                    "compact-sessions-all",
                    "compact-sessions-codex-details",
                    "compact-sessions-fork",
                    "compact-sessions-fork-choice",
                    "compact-sessions-new",
                    "compact-sessions-new-machine",
                    "compact-sessions-new-no-match",
                    "compact-sessions-new-returned",
                    "compact-sessions-machine-sources",
                    "compact-sessions-client-no-match",
                    "compact-sessions-chooser",
                    "compact-sessions-help",
                    "compact-sessions-claude",
                    "compact-sessions-cancelled",
                    "compact-sessions-details",
                    "compact-sessions-folder-choice",
                    "compact-sessions-model-provider-choice",
                    "compact-sessions-source-choice",
                    "compact-sessions-runtime-choice",
                    "compact-sessions-sort-choice",
                ]
                .as_slice(),
            ),
            (
                CapturedTuiSurface::Quota,
                [
                    "compact-quota-all",
                    "compact-quota-provider-no-match",
                    "compact-quota-chooser",
                    "compact-quota-help",
                    "compact-quota-disabled-choice",
                    "compact-quota-hidden-choice",
                    "compact-quota-claude",
                    "compact-quota-cancelled",
                    "compact-quota-details",
                ]
                .as_slice(),
            ),
        ] {
            for (columns, rows) in crate::CURRENT_STATIC_VIEWPORTS.into_iter().rev() {
                let (tape, checkpoints) = build_compact_filter_tape(surface, columns, rows);
                let measured: &[&str] = match (columns, rows) {
                    (32, 24) | (40, 24) => &[
                        "compact-sessions-details-title",
                        "compact-sessions-details",
                        "compact-sessions-details-path",
                    ],
                    (48, 12) => &[
                        "compact-sessions-details-title",
                        "compact-sessions-details-identity",
                        "compact-sessions-details-state",
                        "compact-sessions-details-path",
                    ],
                    (48, 24) => &["compact-sessions-details-title", "compact-sessions-details"],
                    _ => &["compact-sessions-details"],
                };
                let expected: Vec<&str> = expected
                    .iter()
                    .flat_map(|name| {
                        if *name == "compact-sessions-details" {
                            measured.to_vec()
                        } else {
                            vec![*name]
                        }
                    })
                    .collect();
                assert_eq!(
                    checkpoints, expected,
                    "each declared menu/Details pose needs its own original"
                );
                validate_compact_checkpoints(surface, columns, rows, &checkpoints)
                    .expect("the real capture consumer must accept every declared checkpoint");
                assert_eq!(tape.matches("Screenshot ").count(), expected.len());
                assert!(tape.contains("Wait+Screen /QUOTA_STORYBOARD_CHILD_EXIT=0/"));
                // Exact ordered equality above rejects unknown scenes while the
                // adopted journey may legitimately grow to cover more menus.
            }
        }
    }

    #[test]
    fn quota_no_match_wait_accepts_wrapped_words_but_rejects_wrong_content() -> TestResult<()> {
        let (tape, _) = build_compact_filter_tape(CapturedTuiSurface::Quota, 48, 24);
        let wait = tape
            .lines()
            .find(|line| line.starts_with("Wait+Screen /(?s)Account filters.*Account provider[:]?"))
            .ok_or("no-match wait missing")?;
        let pattern = wait
            .strip_prefix("Wait+Screen /")
            .and_then(|line| line.strip_suffix('/'))
            .ok_or("no-match pattern missing")?;
        // This common regex subset preserves the exact ordered words, allowing
        // only whitespace/frame separators actually seen in the failed PTY.
        let result = std::process::Command::new("/usr/bin/python3").args(["-c", r#"
import re, sys
pattern = re.compile(sys.argv[1])
prefix = "Account filters Account provider Search: zznomatchzz "
valid = [prefix + "No matching choices",
         prefix + "\n│ ││                    No matching       ┃ ││ │\n│ ││                    choices           │ ││ │",
         prefix + "\n│ A│                    No matching       ┃ │  │\n│ ┌│                    choices           │ │┐ │",
         "Account filters Account provider\n│ Search: zznomatchzz No matching choices"]
invalid = [prefix + "No matching results", prefix + "choices No matching",
           prefix + "No matching", prefix + "No matching other choices",
           prefix + "No matching Ctrl+F choices",
           prefix + "No matching" + " " * 129 + "choices",
           "Account filters Search: zznomatchzz No matching choices",
           "Account filters Account provider Search: another No matching choices"]
assert all(pattern.search(text) for text in valid), "wrapped exact label must match"
assert not any(pattern.search(text) for text in invalid), "wrong content must reject"
"#, pattern]).output()?;
        if !result.status.success() {
            return Err(std::io::Error::other(
                String::from_utf8_lossy(&result.stderr).into_owned(),
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn quota_draft_capture_requires_labeled_values_without_offscreen_summary() {
        for (columns, rows) in [(120, 36), (76, 30), (48, 24), (48, 12)] {
            let (tape, _) = build_compact_filter_tape(CapturedTuiSurface::Quota, columns, rows);
            assert!(
                !tape.contains("Draft: OpenAI"),
                "offscreen decorative heading cannot own readiness"
            );
            assert!(tape.contains(r"Account provider[ \t]+OpenAI.*Disabled accounts[ \t]+Shown"));
            assert!(tape.contains("Down\nWait+Screen /(?s)Account filters.*Account provider"));
            assert!(
                tape.contains("Up\nF1\n"),
                "restore provider focus before existing Help/edit journey"
            );
            assert!(tape.contains("Escape\nWait+Screen /(?s)Account filters.*Account provider"));
        }
    }

    #[test]
    fn quota_capture_requires_exact_provider_disabled_and_count_variants() {
        for (columns, rows) in [(120, 36), (76, 30), (48, 24)] {
            let (tape, checkpoints) =
                build_compact_filter_tape(CapturedTuiSurface::Quota, columns, rows);
            assert_eq!(checkpoints.len(), 9);
            // The adopted row contract keeps full names at narrow widths too.
            assert!(tape.contains("All.*Disabled: Shown.*4\\/4( accounts| ▾).*Claude Delta"));
            assert!(tape.contains("Claude.*Disabled: Hidden.*1\\/4( accounts| ▾).*Claude Gamma"));
            assert!(tape.contains("Ctrl+F\nEnter\nType \"zznomatchzz\""));
            assert!(tape.contains(r"No matching[ \t\r\n│┃┌┐]{1,128}choices"));
            assert!(tape.contains("Quota Help.*Move field"));
            assert!(tape.contains("Account details.*Claude Gamma"));
            assert!(tape.contains("QUOTA_STORYBOARD_CHILD_EXIT=0"));
            assert!(!tape.contains("Wait+Screen /4\\/4 accounts/"));
        }
    }
}
