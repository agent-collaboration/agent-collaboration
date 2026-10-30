//! Opt-in real quota screen capture. No production account or native Host is used.

#![cfg(unix)]

#[path = "quota_storyboard_capture_test/mod.rs"]
mod capture_support;
#[path = "quota_reset_pty_test/isolated_fixture_test.rs"]
#[expect(
    dead_code,
    reason = "Read-only shared fixture also exposes cached-credit seeding, outside this journey"
)]
mod isolated_fixture_test;
#[path = "quota_reset_pty_test/loopback_provider_test.rs"]
#[expect(
    dead_code,
    reason = "Read-only shared provider also exposes fault/held-response scenarios, outside this journey"
)]
mod loopback_provider_test;

#[path = "quota_storyboard_capture_test/grid_terminal_capture.rs"]
mod grid_terminal_capture;
#[path = "quota_reset_pty_test/terminal_interaction_test.rs"]
#[expect(
    dead_code,
    reason = "Shared PTY owner retains the existing semantic-wait API for its original tests"
)]
mod terminal_interaction_test;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a declared grid-observer target and an interactive driver protocol"]
async fn compiled_grid_terminal_observation() -> TestResult<()> {
    grid_terminal_capture::run_grid_terminal().await
}

use isolated_fixture_test::TestResult;

const CURRENT_STATIC_VIEWPORTS: [(u16, u16); 6] =
    [(120, 36), (76, 30), (48, 24), (48, 12), (40, 24), (32, 24)];

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicitly nominated isolated VHS/ttyd tools and output directory"]
async fn compiled_quota_storyboard_drives_real_ui_and_exports_screens() -> TestResult<()> {
    capture_support::run_storyboard(120, 36, false).await?;
    capture_support::run_storyboard(76, 30, false).await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicitly nominated isolated VHS/ttyd tools and output directory"]
async fn compiled_quota_storyboard_rejects_an_impossible_screen() -> TestResult<()> {
    capture_support::run_storyboard(120, 36, true).await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicitly nominated isolated VHS/ttyd tools and output directory"]
async fn compiled_quota_storyboard_cleans_an_interrupted_live_capture() -> TestResult<()> {
    capture_support::run_interrupted_capture_cleanup().await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires nominated private capture tools and fresh owned output directory"]
async fn compiled_provider_filter_storyboards_export_both_real_tuis() -> TestResult<()> {
    for (columns, rows) in [(120, 36), (76, 30), (48, 24)] {
        for journey in [
            capture_support::ProviderFilterJourney::QuotaMixed,
            capture_support::ProviderFilterJourney::Sessions,
            capture_support::ProviderFilterJourney::QuotaEmpty,
        ] {
            capture_support::run_provider_filter_storyboard(columns, rows, journey).await?;
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires admitted private current-source tools, target and fresh output directory"]
async fn compiled_compact_current_storyboard_exports_both_real_tuis() -> TestResult<()> {
    for (columns, rows) in CURRENT_STATIC_VIEWPORTS {
        for journey in [
            capture_support::ProviderFilterJourney::CompactSessions,
            capture_support::ProviderFilterJourney::CompactQuota,
        ] {
            capture_support::run_provider_filter_storyboard(columns, rows, journey).await?;
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires admitted private current-source tools, target and fresh output directory"]
async fn compiled_compact_current_quota_storyboard_exports_real_tui() -> TestResult<()> {
    for (columns, rows) in CURRENT_STATIC_VIEWPORTS {
        capture_support::run_provider_filter_storyboard(
            columns,
            rows,
            capture_support::ProviderFilterJourney::CompactQuota,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires admitted private current-source tools, target and fresh output directory"]
async fn compiled_compact_current_sessions_storyboard_exports_real_tui() -> TestResult<()> {
    for (columns, rows) in CURRENT_STATIC_VIEWPORTS.into_iter().rev() {
        capture_support::run_provider_filter_storyboard(
            columns,
            rows,
            capture_support::ProviderFilterJourney::CompactSessions,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a released exact component libtest, source-bound tools and fresh output"]
async fn compiled_normal_quota_component_dialogs_export_real_tui() -> TestResult<()> {
    let grids = std::env::var("QUOTA_STORYBOARD_COMPONENT_VIEWPORTS")
        .unwrap_or_else(|_| "120x36,76x30,48x24,48x12,40x24,32x24".to_owned());
    // Validate the complete request before starting even the first UI child.
    for (columns, rows) in declared_component_viewports(&grids)? {
        capture_support::run_normal_component_capture(columns, rows).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a released exact component libtest, source-bound tools and fresh output"]
async fn compiled_hard_case_quota_component_exports_real_tui() -> TestResult<()> {
    // 48x12 uses its own measured one-fact poses (quota_component_tape.rs).
    let grids = declared_component_viewports(
        &std::env::var("QUOTA_STORYBOARD_COMPONENT_VIEWPORTS")
            .unwrap_or_else(|_| "120x36,76x30,48x24,48x12,40x24,32x24".to_owned()),
    )?;
    for (columns, rows) in grids {
        capture_support::run_hard_case_component_capture(columns, rows).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires the admitted48x12 first-frame exit-sync diagnostic target and fresh owned output"]
async fn compiled_quota_phone_first_frame_diagnostic_exports_original() -> TestResult<()> {
    capture_support::run_first_frame_component_diagnostic().await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires the admitted fixed natural-error control and fresh owned output"]
async fn compiled_quota_natural_error_retains_original_raster_layers() -> TestResult<()> {
    capture_support::run_component_failure_retention_control().await
}

fn declared_component_viewports(value: &str) -> TestResult<Vec<(u16, u16)>> {
    let mut seen = std::collections::HashSet::new();
    value
        .split(',')
        .map(|grid| {
            let viewport = match grid {
                "120x36" => (120, 36),
                "76x30" => (76, 30),
                "48x24" => (48, 24),
                "48x12" => (48, 12),
                "40x24" => (40, 24),
                "32x24" => (32, 24),
                _ => return Err("component capture viewport is not declared".into()),
            };
            if !seen.insert(viewport) {
                return Err("duplicate component capture viewport".into());
            }
            Ok(viewport)
        })
        .collect()
}

#[test]
fn component_viewports_reject_invalid_or_duplicate_requests_before_capture() {
    assert_eq!(
        declared_component_viewports("120x36,76x30,48x24,48x12,40x24,32x24")
            .expect("the six nominated viewports are valid"),
        CURRENT_STATIC_VIEWPORTS
    );
    assert_eq!(
        declared_component_viewports("32x24").expect("the nominated phone viewport is valid"),
        [(32, 24)]
    );
    for invalid in ["", "120x36,40x12", "32x24,32x24", "40x24,32x12"] {
        assert!(declared_component_viewports(invalid).is_err(), "{invalid}");
    }
}
