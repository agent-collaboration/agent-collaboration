//! Literal C1–C7 contracts at the actual row and mounted controller boundaries.
use super::*;
use codex_router_quota_service::quota_display_model::QuotaDisplayWindow;

#[test]
fn folded_groups_wrap_full_unicode_identity_without_internal_blank_rows() {
    for width in [76usize, 48, 40, 32] {
        let mut model = quota_two_account_view_model();
        let row = &mut model.rows[0];
        row.account = "東京 Alpha Research Account Long Original Name".to_owned();
        let frame =
            super::super::quota_account_groups::render_interactive_account(row, width, false)
                .render(None)
                .to_string();
        let normalized = frame.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("東京 Alpha Research Account Long Original Name"),
            "C1 whole name at {width}: {frame}"
        );
        let headings = ["Account", "Quota left · reset in", "Weekly pace", "Extra"];
        let mut previous = 0;
        for heading in headings {
            let index = frame
                .find(heading)
                .unwrap_or_else(|| panic!("{heading} at {width}: {frame}"));
            assert!(index >= previous, "{frame}");
            previous = index;
        }
        assert!(
            frame.lines().all(|line| !line.trim().is_empty()),
            "C5 zero empty rows inside account: {frame}"
        );
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "{frame}"
        );
    }
}

#[test]
fn compact_windows_keep_known_amount_when_reset_is_unknown_and_missing_has_no_meter() {
    let mut model = quota_two_account_view_model();
    let row = &mut model.rows[0];
    row.display_weekly = QuotaDisplayWindow::Known {
        remaining_percent: 92,
        reset_in_seconds: None,
        stale: false,
    };
    row.display_short = QuotaDisplayWindow::Unknown;
    row.weekly_window = "DO NOT PARSE THIS".to_owned();
    row.short_window = "100% fabricated".to_owned();
    let canvas = super::super::quota_account_groups::render_interactive_account(row, 110, false)
        .render(None);
    let frame = canvas.to_string();
    assert!(
        frame.contains("92% · week") && !frame.contains("· —"),
        "{frame}"
    );
    assert!(
        !frame.contains("No data") && !frame.contains("5h"),
        "{frame}"
    );
    assert!(
        !frame.contains("DO NOT") && !frame.contains("fabricated") && !frame.contains("Resets"),
        "{frame}"
    );
    assert_eq!(
        frame
            .chars()
            .filter(|glyph| matches!(*glyph, '█' | '░'))
            .count(),
        7,
        "only the known weekly meter remains: {frame}"
    );
}

#[tokio::test]
async fn pool_f5_census_and_modal_precedence_restore_same_account() {
    let mut model = quota_two_account_view_model();
    model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
        usable: 2,
        reserve: 3,
        blocked: 4,
        unknown: 5,
        excluded: 0,
    };
    model.header.freshness =
        codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
            fresh: 6,
            stale: 7,
            unknown: 8,
        };
    let frames = render_quota_filter_phases(
        model,
        120,
        36,
        vec![
            (
                "Quota status",
                vec![
                    provider_filter_key(KeyCode::Down),
                    provider_filter_key(KeyCode::F(5)),
                ],
            ),
            (
                "Pool details",
                vec![
                    provider_filter_key(KeyCode::Enter),
                    TerminalEvent::Key({
                        let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('r'));
                        key.modifiers = KeyModifiers::CONTROL;
                        key
                    }),
                    provider_filter_key(KeyCode::Esc),
                ],
            ),
            ("Filters", vec![provider_filter_key(KeyCode::Enter)]),
            (
                "Account details",
                vec![
                    provider_filter_key(KeyCode::F(5)),
                    provider_filter_key(KeyCode::Esc),
                ],
            ),
            ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    let pool = frames
        .iter()
        .find(|frame| frame.contains("Accounts: 14"))
        .expect("F5 opens read-only census");
    for fact in [
        "Usable: 2",
        "Reserve: 3",
        "Blocked: 4",
        "Pool unknown: 5",
        "Excluded: 0",
        "Fresh: 6",
        "Stale: 7",
        "Data unknown: 8",
    ] {
        assert!(pool.contains(fact), "{pool}");
    }
    assert!(
        !frames
            .iter()
            .any(|frame| frame.contains("Options · Resets")),
        "account effects blocked in pool"
    );
    let account = frames
        .iter()
        .find(|frame| frame.contains("Account details"))
        .unwrap();
    assert!(account.contains("beta"), "{account}");
}

#[tokio::test]
async fn f5_behind_account_or_filter_modal_preserves_that_active_snapshot() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let terminal_width_reader: fn() -> Option<usize> = || None;
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:0usize,terminal_width_reader:Some(terminal_width_reader),height:0usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        sender.send(TerminalEvent::Resize(120, 36)).unwrap();
        super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 120
                    && canvas.height() == 36
                    && canvas.to_string().contains("Quota status")
            },
        )
        .await
        .unwrap();
        sender.send(provider_filter_key(KeyCode::Down)).unwrap();
        super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 120
                    && canvas.height() == 36
                    && canvas
                        .to_string()
                        .lines()
                        .any(|line| line.contains('❯') && line.contains("beta"))
            },
        )
        .await
        .unwrap();
        sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
        let account = super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 120
                    && canvas.height() == 36
                    && canvas.to_string().contains("Account details")
                    && canvas.to_string().contains("beta-tag")
            },
        )
        .await
        .unwrap();
        assert!(account.to_string().contains("Account details"));
        sender.send(provider_filter_key(KeyCode::F(5))).unwrap();
        sender.send(TerminalEvent::Resize(76, 30)).unwrap();
        let account = super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 76
                    && canvas.height() == 30
                    && canvas.to_string().contains("Account details")
                    && canvas.to_string().contains("beta-tag")
            },
        )
        .await
        .unwrap();
        assert_eq!(account.width(), 76);
        let account = super::quota_ux_clarity_test::actual_dialog_text(
            &account.to_string(),
            "Account details",
        );
        assert!(
            account.contains("beta"),
            "F5 preserves captured Account modal: {account}"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 76
                    && canvas.height() == 30
                    && !canvas.to_string().contains("Account details")
                    && canvas
                        .to_string()
                        .lines()
                        .any(|line| line.contains('❯') && line.contains("beta"))
            },
        )
        .await
        .unwrap();
        sender.send(provider_filter_key(KeyCode::F(3))).unwrap();
        super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 76
                    && canvas.height() == 30
                    && canvas.to_string().contains("Account filters")
                    && canvas.to_string().contains("Account provider")
            },
        )
        .await
        .unwrap();
        sender.send(provider_filter_key(KeyCode::F(5))).unwrap();
        sender.send(TerminalEvent::Resize(120, 36)).unwrap();
        let filters = super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| {
                canvas.width() == 120
                    && canvas.height() == 36
                    && canvas.to_string().contains("Account filters")
                    && canvas.to_string().contains("Account provider")
                    && canvas.to_string().contains("Disabled accounts")
            },
        )
        .await
        .unwrap();
        assert_eq!(filters.width(), 120);
        let filters = super::quota_ux_clarity_test::actual_dialog_text(
            &filters.to_string(),
            "Account filters",
        );
        assert!(
            filters.contains("Account provider") && filters.contains("Disabled accounts"),
            "F5 preserves filter modal: {filters}"
        );
    };
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .unwrap();
}

#[test]
fn quota_caller_binds_week_and_short_amount_label_and_meter_to_distinct_tones() {
    let mut model = quota_two_account_view_model();
    let row = &mut model.rows[0];
    row.account = "Tone fixture".to_owned();
    row.display_weekly = QuotaDisplayWindow::Known {
        remaining_percent: 92,
        reset_in_seconds: Some(576000),
        stale: false,
    };
    row.display_short = QuotaDisplayWindow::Known {
        remaining_percent: 83,
        reset_in_seconds: Some(7800),
        stale: false,
    };
    let canvas = super::super::quota_account_groups::render_interactive_account(row, 110, false)
        .render(None);
    let frame = canvas.to_string();
    for (percent, label, expected) in [
        (
            "92%",
            "week",
            Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
        ),
        (
            "83%",
            "5h",
            Color::Rgb {
                r: 206,
                g: 212,
                b: 221,
            },
        ),
    ] {
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(percent))
            .unwrap();
        for token in [percent, label, "█"] {
            let column = unicode_width::UnicodeWidthStr::width(line.split_once(token).unwrap().0);
            assert_eq!(
                canvas
                    .cell(column, row)
                    .unwrap()
                    .text_style()
                    .unwrap()
                    .color,
                Some(expected),
                "C3 caller binding {token}: {frame}"
            );
        }
    }
}

#[tokio::test]
async fn mounted_quota_only_selection_is_filled_and_name_origin_stays_fixed() {
    let mut model = quota_many_account_view_model();
    model.rows.truncate(3);
    for (index, row) in model.rows.iter_mut().enumerate() {
        row.selected = index == 2;
    }
    let mut component =
        element!(QuotaStatusComponent(view_model:model,width:120usize,height:36usize));
    let canvases = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            [provider_filter_key(KeyCode::Esc)],
        )))
        .collect::<Vec<_>>()
        .await;
    let canvas = canvases.last().unwrap();
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Account") && line.contains("Extra"))
        .unwrap();
    let column = unicode_width::UnicodeWidthStr::width(line.split_once("Account").unwrap().0);
    assert_eq!(canvas.cell(column, row).unwrap().background_color, None);
    assert_eq!(
        canvas
            .cell(column, row)
            .unwrap()
            .text_style()
            .unwrap()
            .color,
        Some(Color::Rgb {
            r: 237,
            g: 240,
            b: 245
        })
    );
    let mut origins = Vec::new();
    for (account, expected) in [
        ("acct00", None),
        ("acct01", None),
        (
            "acct02",
            Some(Color::Rgb {
                r: 52,
                g: 59,
                b: 70,
            }),
        ),
    ] {
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(account))
            .unwrap();
        let column = unicode_width::UnicodeWidthStr::width(line.split_once(account).unwrap().0);
        origins.push(column);
        assert_eq!(
            canvas.cell(column, row).unwrap().background_color,
            expected,
            "C4 actual account fill: {frame}"
        );
    }
    assert!(
        origins.windows(2).all(|pair| pair[0] == pair[1]),
        "C4 selection keeps name origin: {frame}"
    );
}

#[test]
fn quota_account_background_is_uniform_at_wide_and_folded_widths() {
    let model = quota_two_account_view_model();
    for width in [120, 76, 48, 40, 32] {
        for selected in [false, true] {
            let canvas = super::super::quota_account_groups::render_interactive_account(
                &model.rows[0],
                width,
                selected,
            )
            .render(None);
            let expected = selected.then_some(Color::Rgb {
                r: 52,
                g: 59,
                b: 70,
            });
            for row in 0..canvas.height() {
                for column in 0..canvas.width() {
                    assert_eq!(
                        canvas.cell(column, row).unwrap().background_color,
                        expected,
                        "No heading bands or alternating fills at {width}, selected={selected}, cell={column},{row}"
                    );
                }
            }
        }
    }
}

const POOL_CENSUS_FACTS: [&str; 10] = [
    "Accounts: 14",
    "Usable: 2",
    "Reserve: 3",
    "Blocked: 4",
    "Pool unknown: 5",
    "Excluded: 0",
    "Fresh: 6",
    "Stale: 7",
    "Data unknown: 8",
    "Clients: 9",
];

fn observed_pool_body_facts(frames: &[String]) -> std::collections::BTreeSet<&'static str> {
    let mut observed = std::collections::BTreeSet::new();
    for frame in frames {
        // The browse header also says Pool details. Preserve every cell/border,
        // but mask earlier navigation-label occurrences before binding the modal.
        let Some(title) = frame.rfind("Pool details") else {
            return std::collections::BTreeSet::new();
        };
        let (underlay, modal) = frame.split_at(title);
        let scoped = format!(
            "{}{modal}",
            underlay.replace("Pool details", "            ")
        );
        let dialog = super::quota_ux_clarity_test::actual_dialog_text(&scoped, "Pool details");
        let rows = dialog.lines().collect::<Vec<_>>();
        if !rows.iter().any(|row| row.trim() == "Pool details") {
            return std::collections::BTreeSet::new();
        }
        let footer = rows
            .iter()
            .position(|row| row.contains("Close"))
            .expect("Pool Close stays inside the actual modal bounds");
        // Exclude the fixed title and footer. Match each entire literal fact row,
        // removing only whitespace and the body's one-cell scrollbar track.
        for row in &rows[1..footer] {
            let row = row.trim_matches([' ', '│']);
            for fact in POOL_CENSUS_FACTS {
                if row == fact {
                    observed.insert(fact);
                }
            }
        }
    }
    observed
}

#[tokio::test]
async fn pool_census_all_ten_facts_are_reachable_in_body_at_six_grids_and_restore_beta() {
    let mut short_observations = Vec::new();
    let mut short_browse_frame = String::new();
    for (width, height) in [
        (120usize, 36usize),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
    ] {
        // Arrange a real mounted controller at this physical grid. Its selection
        // and modal stay in this one event stream throughout scrolling and Esc.
        let mut model = quota_two_account_view_model();
        model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
            usable: 2,
            reserve: 3,
            blocked: 4,
            unknown: 5,
            excluded: 0,
        };
        model.header.freshness =
            codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
                fresh: 6,
                stale: 7,
                unknown: 8,
            };
        model.header.observed_clients = Some(9);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut last_frame = String::new();
        let journey = async {
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            let mut canvas =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!((canvas.width(), canvas.height()), (width, height));
            let browse_before = canvas.to_string();

            // Act: F5 opens Pool, then actual navigation reveals every body row.
            sender.send(provider_filter_key(KeyCode::F(5))).unwrap();
            canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            let mut observations = Vec::new();
            for step in 0..=16 {
                assert_eq!((canvas.width(), canvas.height()), (width, height));
                last_frame = canvas.to_string();
                println!("POOL ORIGINAL FRAME {width}x{height} step{step}:\n{last_frame}");
                observations.push(last_frame.clone());
                let observed = observed_pool_body_facts(&observations);
                println!("POOL BODY {width}x{height} step{step}: {observed:?}");
                if observed.len() == POOL_CENSUS_FACTS.len() {
                    break;
                }
                assert!(
                    step < 16,
                    "unreachable Pool facts at {width}x{height}: {observed:?}\n{last_frame}"
                );
                let key = if step % 3 == 2 {
                    KeyCode::PageDown
                } else {
                    KeyCode::Down
                };
                sender.send(provider_filter_key(key)).unwrap();
                canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            }
            assert_eq!(
                observed_pool_body_facts(&observations),
                POOL_CENSUS_FACTS.into_iter().collect()
            );
            if (width, height) == (48, 12) {
                short_observations = observations;
                short_browse_frame = browse_before.clone();
            }

            // Assert: Esc restores the exact browse frame; F4 independently
            // witnesses the same captured Beta identity inside Account details.
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                canvas.to_string(),
                browse_before,
                "Pool changed browse selection/filter at {width}x{height}"
            );
            sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
            canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            let account = super::quota_ux_clarity_test::actual_dialog_text(
                &canvas.to_string(),
                "Account details",
            );
            assert!(
                account.split_whitespace().any(|word| word == "beta"),
                "Pool lost captured Beta at {width}x{height}: {account}"
            );
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
        };
        tokio::time::timeout(Duration::from_secs(3), journey).await.unwrap_or_else(|_| panic!("Pool real viewport journey stalled at {width}x{height}; last frame:\n{last_frame}"));
    }

    // Challenge the same oracle with actual short-grid frames: top/bottom alone
    // miss middle facts, and underlay text cannot replace a missing body row.
    let endpoints = vec![
        short_observations.first().unwrap().clone(),
        short_observations.last().unwrap().clone(),
    ];
    assert!(
        observed_pool_body_facts(&endpoints).len() < POOL_CENSUS_FACTS.len(),
        "48x12 endpoint images must not stand in for the middle rows"
    );
    assert!(
        observed_pool_body_facts(&[short_browse_frame]).is_empty(),
        "header Pool action without a Pool modal must not bind as a Pool body"
    );
    let wrong_dialog = short_observations
        .iter()
        .map(|frame| {
            let mut frame = frame.clone();
            let title = frame.rfind("Pool details").unwrap();
            frame.replace_range(title..title + "Pool details".len(), "Account info");
            frame
        })
        .collect::<Vec<_>>();
    assert!(
        observed_pool_body_facts(&wrong_dialog).is_empty(),
        "all census facts in another titled modal must not count as Pool facts"
    );
    for fact in POOL_CENSUS_FACTS {
        let missing = short_observations
            .iter()
            .map(|frame| frame.replace(fact, &" ".repeat(fact.len())))
            .collect::<Vec<_>>();
        assert_eq!(
            observed_pool_body_facts(&missing).len(),
            9,
            "missing {fact}"
        );
        let outside = missing
            .iter()
            .map(|frame| format!("{frame}\n{fact}"))
            .collect::<Vec<_>>();
        assert!(outside.iter().all(|frame| frame.contains(fact)));
        assert!(
            !observed_pool_body_facts(&outside).contains(fact),
            "outside-modal {fact} replaced a missing body row"
        );
        let (label, value) = fact.rsplit_once(' ').unwrap();
        let wrong = format!(
            "{label} {}",
            if value == "9" {
                "0".to_owned()
            } else {
                "9".repeat(value.len())
            }
        );
        let wrong_frames = short_observations
            .iter()
            .map(|frame| frame.replace(fact, &wrong))
            .collect::<Vec<_>>();
        assert!(
            !observed_pool_body_facts(&wrong_frames).contains(fact),
            "wrong value replaced {fact}"
        );
    }
    println!(
        "POOL ORACLE CHALLENGES: all10 missing/outside/wrong pairs rejected; header-only, wrong-dialog and short endpoint-only rejected"
    );
}

#[test]
fn unknown_reset_keeps_known_amount_and_staleness_in_actual_row() {
    for (reset_in_seconds, expected) in [(None, "5% · week"), (Some(0), "5% · week · 0m")] {
        let mut model = quota_two_account_view_model();
        let row = &mut model.rows[0];
        row.display_weekly = QuotaDisplayWindow::Known {
            remaining_percent: 5,
            reset_in_seconds,
            stale: true,
        };
        let frame = super::super::quota_account_groups::render_interactive_account(row, 120, false)
            .render(None)
            .to_string();
        assert!(frame.contains(expected), "{frame}");
        assert!(frame.contains("week stale"), "{frame}");
    }
}

#[tokio::test]
async fn pool_help_names_its_parent_and_restores_selected_account() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap();
        sender.send(provider_filter_key(KeyCode::Down)).unwrap();
        let browse = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        sender.send(provider_filter_key(KeyCode::F(5))).unwrap();
        let pool = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        assert!(pool.contains("Accounts:"), "actual Pool body: {pool}");
        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
        let help = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        let help = super::quota_ux_clarity_test::actual_dialog_text(&help, "Quota Help");
        assert!(
            help.lines().any(|line| line.trim() == "Pool details"),
            "Pool context title: {help}"
        );
        assert!(
            !help.lines().any(|line| line.trim() == "Account details"),
            "wrong Account context: {help}"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        let restored_pool =
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap()
                .to_string();
        assert_eq!(
            restored_pool, pool,
            "Help must return to the same Pool body"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        let restored_browse =
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap()
                .to_string();
        assert_eq!(restored_browse, browse, "Pool Help must retain selection");
        sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
        let account = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        let account = super::quota_ux_clarity_test::actual_dialog_text(&account, "Account details");
        assert!(
            account.split_whitespace().any(|word| word == "beta"),
            "captured Beta: {account}"
        );
    };
    tokio::time::timeout(Duration::from_secs(3), journey)
        .await
        .expect("bounded F5 Help return journey");
}
