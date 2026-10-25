//! Contracts and retained-screen controls for the compact capture tape.
use super::*;

#[test]
fn collapsed_filter_waits_bind_inline_and_stacked_field_values() -> TestResult<()> {
    let (tape, _) = build_compact_filter_tape(CapturedTuiSurface::Quota, 48, 24);
    let waits = tape
        .lines()
        .filter(|line| {
            line.starts_with("Wait+Screen /(?s)Account filters.*Account provider")
                && line.contains("OpenAI")
        })
        .collect::<Vec<_>>();
    ensure(
        waits.len() == 3,
        "chooser, Help return and cancelled draft waits",
    )?;
    let disabled_ready = format!(r"Disabled accounts{COLLAPSED_FILTER_VALUE_GAP}Shown\b");
    ensure(
        waits
            .iter()
            .filter(|wait| wait.contains(&disabled_ready))
            .count()
            == 1,
        "exactly the chooser wait must require the Disabled accounts value",
    )?;
    // Literal collapsed fields from the retained short-width terminal frame.
    let stacked = "│ ││Account filters                     Help││ │\n\
│ ││Account provider                        ││ │\n\
│ ││  OpenAI ▾                              ││ │\n\
│ ││Disabled accounts                       ││ │\n\
│ ││  Shown ▾                               ││ │";
    let inline = "Account filters\nAccount provider OpenAI ▾\nDisabled accounts Shown ▾";
    for wait in waits {
        let pattern = wait
            .strip_prefix("Wait+Screen /")
            .and_then(|line| line.strip_suffix('/'))
            .ok_or("missing field wait")?;
        let pattern = super::super::vhs_wait_pattern_oracle::wait_pattern(pattern)?;
        let positives = [
            inline.to_owned(),
            stacked.to_owned(),
            super::super::vhs_wait_pattern_oracle::right_padded_rows(stacked, 2400),
            format!(
                "Account filters\nAccount provider{}OpenAI\nDisabled accounts{}Shown",
                " ".repeat(4096),
                " ".repeat(4096),
            ),
        ];
        let mut negatives = vec![
            stacked.replace("OpenAI", "Claude"),
            stacked.replace("OpenAI", "OpenAIextra"),
            stacked.replace("Account provider", "Session client"),
            stacked.replace("  OpenAI ▾", "  ▾"),
            stacked.replace("  OpenAI ▾", "Other field\n  OpenAI ▾"),
            "Account filters\nAccount providerOpenAI\nDisabled accounts Shown".to_owned(),
            format!(
                "Account filters\nAccount provider{}OpenAI\nDisabled accounts Shown",
                " ".repeat(4097),
            ),
        ];
        if wait.contains("Disabled accounts") {
            negatives.push(stacked.replace("Shown", "Hidden"));
            negatives.push(stacked.replace("Disabled accounts", "Other accounts"));
            negatives.push(format!(
                "Account filters\nAccount provider OpenAI\nDisabled accounts{}Shown",
                " ".repeat(4097),
            ));
        }
        super::super::vhs_wait_pattern_oracle::ensure_wait_shapes(
            &pattern, &positives, &negatives,
        )?;
    }
    Ok(())
}

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
        let (_, checkpoints) = build_compact_filter_tape(CapturedTuiSurface::Quota, columns, rows);
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
    assert!(validate_compact_checkpoints(CapturedTuiSurface::Quota, 120, 36, &replaced).is_err());
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
                "compact-quota-pool-details",
                "compact-quota-pool-details-complete",
                "compact-quota-pool-details-middle",
                "compact-quota-pool-details-bottom",
                "compact-quota-pool-closed",
                "compact-quota-filters-block-pool",
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
            // Measured Details order: fields,
            // path, title at every grid; 48x12 identity, state, path, title.
            let measured: &[&str] = match (columns, rows) {
                (48, 12) => &[
                    "compact-sessions-details-identity",
                    "compact-sessions-details-state",
                    "compact-sessions-details-path",
                    "compact-sessions-details-title",
                ],
                _ => &[
                    "compact-sessions-details",
                    "compact-sessions-details-path",
                    "compact-sessions-details-title",
                ],
            };
            let expected: Vec<&str> = expected
                .iter()
                .flat_map(|name| {
                    if *name == "compact-sessions-details" {
                        measured.to_vec()
                    } else if *name == "compact-quota-pool-details-complete" && rows < 36 {
                        // The whole census body fits only at 36 rows.
                        vec![]
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
    let pattern = super::super::vhs_wait_pattern_oracle::wait_pattern(pattern)?;
    let prefix = "Account filters Account provider Search: zznomatchzz▏ ";
    // Wrapped exact label must match.
    let valid = [
        format!("{prefix}No matching choices"),
        format!(
            "{prefix}\n│ ││                    No matching       ┃ ││ │\n│ ││                    choices           │ ││ │"
        ),
        format!(
            "{prefix}\n│ A│                    No matching       ┃ │  │\n│ ┌│                    choices           │ │┐ │"
        ),
        "Account filters Account provider\n│ Search: zznomatchzz▏ No matching choices".to_owned(),
    ];
    // Wrong content must reject.
    let invalid = [
        format!("{prefix}No matching results"),
        format!("{prefix}choices No matching"),
        format!("{prefix}No matching"),
        format!("{prefix}No matching other choices"),
        format!("{prefix}No matching Ctrl+F choices"),
        format!("{prefix}No matching{}choices", " ".repeat(129)),
        "Account filters Account provider Search: zznomatchzz No matching choices".to_owned(),
        "Account filters Search: zznomatchzz▏ No matching choices".to_owned(),
        "Account filters Account provider Search: another▏ No matching choices".to_owned(),
    ];
    super::super::vhs_wait_pattern_oracle::ensure_wait_shapes(&pattern, &valid, &invalid)
}

#[test]
fn quota_draft_capture_requires_labeled_values_without_offscreen_summary() {
    for (columns, rows) in [(120, 36), (76, 30), (48, 24), (48, 12)] {
        let (tape, _) = build_compact_filter_tape(CapturedTuiSurface::Quota, columns, rows);
        assert!(
            !tape.contains("Draft: OpenAI"),
            "offscreen decorative heading cannot own readiness"
        );
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
        assert_eq!(checkpoints.len(), if rows >= 36 { 15 } else { 14 });
        // C6: census lives in F5 Pool details: top, middle and bottom poses
        // at every grid, complete only where it fits (36 rows); F5 under an
        // obscuring filter dialog is exercised separately.
        assert!(tape.contains("F5\nUp@1ms 256\n"));
        assert_eq!(
            tape.contains("Pool details.*Accounts: 4.*Usable: 0.*Reserve: 0.*Blocked: 0.*Pool unknown: 1.*Excluded: 3.*Fresh: 0.*Stale: 0.*Data unknown: 4.*Clients: 0.*Close"),
            rows >= 36
        );
        assert!(tape.contains("Pool details.*Accounts: 4.*Usable: 0.*Reserve: 0.*Close/"));
        assert!(tape.contains("Pool details.*Blocked: 0.*Pool unknown: 1.*Excluded: 3.*Close/"));
        assert!(
            tape.contains("Pool details.*Fresh: 0.*Stale: 0.*Data unknown: 4.*Clients: 0.*Close/")
        );
        assert!(tape.contains("Account provider/\nF5\n"));
        // C7: the exact end caret follows the typed query in terminal text.
        assert!(tape.contains("zznomatchzz▏.*No matching"));
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
