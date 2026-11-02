//! A real committing snapshot makes the existing Options switch unavailable.
use super::*;

#[tokio::test]
async fn quota_committing_switch_keeps_disabled_reason_and_rejects_key_and_pointer_activation() {
    for viewport in STANDARD_GRIDS {
        let state =
            |focus: &str| json!({"tab":"Resets","pending":null,"focused":target(focus).as_str()});
        // Both disabled reasons fit in the 120/76-column switch. The narrower
        // shared width projection hides the entire group and its targets.
        let wide = viewport.0 >= 76;
        let paint = || {
            if wide {
                vec![
                    styled_in(
                        target("resets-tab"),
                        "Resets",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                    styled_in(
                        target("credits-tab"),
                        "Credits",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                    contains("Reset commit in progress"),
                ]
            } else {
                vec![
                    ControlPaint::Absent("Resets".to_owned()),
                    ControlPaint::Absent("Credits".to_owned()),
                ]
            }
        };
        let handlers = ["begin-inspection:test-alpha:1"];
        let mut steps = vec![
            step(
                modified(KeyCode::Char('r'), KeyModifiers::CONTROL),
                true,
                state("domain-body"),
                &handlers,
                paint(),
            ),
            step(
                modified(KeyCode::Tab, KeyModifiers::SHIFT),
                true,
                state("help"),
                &handlers,
                paint(),
            ),
        ];
        if wide {
            steps.extend([
                step(
                    hover(target("credits-tab")),
                    false,
                    state("help"),
                    &handlers,
                    paint(),
                ),
                step(
                    click(target("credits-tab")),
                    false,
                    state("help"),
                    &handlers,
                    paint(),
                ),
            ]);
        }
        assert_control_kind_contract(
            contract(ControlKind::SegmentedSwitch, viewport, steps),
            |journal, observed| {
                fixture::build_committing_switch(
                    crate::quota_test_support::control_kind_contract::ControlInstanceRequest {
                        screen: "quota",
                        kind: ControlKind::SegmentedSwitch,
                        viewport,
                    },
                    journal,
                    observed,
                )
            },
        )
        .await;
    }
}
