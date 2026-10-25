use crate::*;
use iocraft::prelude::*;

#[test]
fn shared_heading_roles_paint_literal_hierarchy() {
    for (level, expected) in [
        (UiHeadingLevel::Surface, Color::Cyan),
        (
            UiHeadingLevel::Section,
            Color::Rgb {
                r: 156,
                g: 188,
                b: 251,
            },
        ),
        (UiHeadingLevel::Group, Color::Grey),
    ] {
        let canvas = element!(UiHeading(content: "Heading", level)).render(None);
        let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
        assert_eq!(style.color, Some(expected));
        assert_eq!(style.weight, Weight::Bold);
    }
    let canvas =
        element!(UiHeading(content: "Background", level: UiHeadingLevel::Surface, inactive: true))
            .render(None);
    assert_eq!(
        canvas.cell(0, 0).unwrap().text_style().unwrap().color,
        Some(Color::Grey)
    );
}

#[test]
fn shared_text_roles_keep_selection_separate_from_hierarchy() {
    for (role, expected_color, expected_weight) in [
        (UiTextRole::Body, Color::White, Weight::Normal),
        (UiTextRole::Secondary, Color::Grey, Weight::Normal),
        (
            UiTextRole::Disabled,
            Color::Rgb {
                r: 166,
                g: 175,
                b: 189,
            },
            Weight::Normal,
        ),
        (UiTextRole::Selected, Color::Yellow, Weight::Bold),
    ] {
        let canvas = element!(UiText(content: "Value", role)).render(None);
        let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
        assert_eq!(style.color, Some(expected_color));
        assert_eq!(style.weight, expected_weight);
    }
}

#[test]
fn shared_action_role_is_white_or_distinct_yellow_focus() {
    for has_focus in [false, true] {
        let canvas = element!(DialogAction(label: "Apply", has_focus)).render(None);
        let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
        assert_eq!(
            style.color,
            Some(if has_focus {
                Color::Yellow
            } else {
                Color::White
            })
        );
        assert_eq!(style.weight, Weight::Bold);
        assert_eq!(style.underline, has_focus);
    }
}

#[test]
fn shared_blocked_action_looks_disabled_and_has_no_focus_decoration() {
    let canvas = element! {
        ContextProvider(value: Context::owned(DialogInputScope::Blocked)) {
            DialogAction(label: "Help", has_focus: true)
        }
    }
    .render(None);
    let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
    assert_eq!(
        style.color,
        Some(Color::Rgb {
            r: 166,
            g: 175,
            b: 189
        })
    );
    assert!(
        !style.underline,
        "blocked background controls cannot advertise active focus"
    );
}

#[test]
fn surface_header_keeps_help_on_the_title_row_at_phone_width() {
    let frame = element! {
        View(width: 44) {
            SurfaceHeader(title: "Sessions · 6/6", help_action: Some(element!(DialogAction(label: "Help")).into_any()))
        }
    }.render(None).to_string();
    assert_eq!(frame.lines().count(), 1, "{frame}");
    assert!(
        frame.contains("Sessions · 6/6") && frame.contains("Help"),
        "{frame}"
    );
    assert!(!frame.contains("F1"), "{frame}");
}

#[test]
fn secondary_help_action_keeps_distinct_focus_and_blocked_styles() {
    for (has_focus, color, weight) in [
        (false, Color::Grey, Weight::Normal),
        (true, Color::Yellow, Weight::Bold),
    ] {
        let canvas = element!(DialogAction(label: "Help", emphasis: DialogActionEmphasis::Secondary, has_focus)).render(None);
        let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
        assert_eq!(style.color, Some(color));
        assert_eq!(style.weight, weight);
        assert_eq!(style.underline, has_focus);
    }
    let canvas = element! {
        ContextProvider(value: Context::owned(DialogInputScope::Blocked)) {
            DialogAction(label: "Help", emphasis: DialogActionEmphasis::Secondary, has_focus: true)
        }
    }
    .render(None);
    let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
    assert_eq!(
        style.color,
        Some(Color::Rgb {
            r: 166,
            g: 175,
            b: 189
        })
    );
    assert!(!style.underline);
}

#[test]
fn surface_header_gap_is_explicit_and_preserves_the_existing_default() {
    for (column_gap, expected) in [
        (None, "3 stale  Details  Help"),
        (Some(1u16), "3 stale Details Help"),
    ] {
        let frame = element! {
            View(width: 44) {
                SurfaceHeader(title: "Quota", column_gap,
                    children: vec![
                        element!(UiText(content: "3 stale", role: UiTextRole::Secondary)).into_any(),
                        element!(DialogAction(label: "Details")).into_any(),
                    ],
                    help_action: Some(element!(DialogAction(label: "Help", emphasis: DialogActionEmphasis::Secondary)).into_any()))
            }
        }.render(None).to_string();
        assert_eq!(frame.lines().count(), 1, "{frame}");
        assert!(
            frame.contains(expected),
            "existing callers keep two cells; compact callers choose one: {frame}"
        );
    }
}

#[test]
fn shared_notice_roles_distinguish_success_warning_and_error_from_selection() {
    for (role, color) in [
        (UiTextRole::Success, Color::Green),
        (
            UiTextRole::Warning,
            Color::Rgb {
                r: 255,
                g: 165,
                b: 0,
            },
        ),
        (UiTextRole::Error, Color::Red),
    ] {
        let canvas = element!(UiText(content:"Explicit notice".to_owned(),role)).render(None);
        let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
        assert_eq!(style.color, Some(color));
        assert_eq!(style.weight, Weight::Normal);
    }
}

#[test]
fn visual_window_roles_and_row_surfaces_follow_the_adopted_palette() {
    for (role, expected) in [
        (
            UiTextRole::WeeklyWindow,
            Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
        ),
        (
            UiTextRole::ShortWindow,
            Color::Rgb {
                r: 206,
                g: 212,
                b: 221,
            },
        ),
    ] {
        let canvas = element!(UiText(content:"92% · week",role)).render(None);
        assert_eq!(
            canvas.cell(0, 0).unwrap().text_style().unwrap().color,
            Some(expected)
        );
    }
    for (selected, hovered, expected) in [
        (
            false,
            false,
            Color::Rgb {
                r: 43,
                g: 48,
                b: 57,
            },
        ),
        (
            false,
            true,
            Color::Rgb {
                r: 48,
                g: 54,
                b: 65,
            },
        ),
        (
            true,
            true,
            Color::Rgb {
                r: 52,
                g: 59,
                b: 70,
            },
        ),
    ] {
        let canvas=element!(View(width:20) {SelectableRow(selected,hovered,surface:UiSurface::Alternate) {Text(content:"Account")}}).render(None);
        assert_eq!(canvas.cell(2, 0).unwrap().background_color, Some(expected));
        assert!(
            canvas
                .to_string()
                .starts_with(if selected { "❯ Account" } else { "  Account" })
        );
    }
}

#[test]
fn column_heading_has_literal_band_white_weight_and_teal_rule() {
    for width in [18u32, 28, 60] {
        let canvas = element!(View(width) {UiColumnHeading(content:"Account")}).render(None);
        let heading = canvas.cell(0, 0).unwrap();
        assert_eq!(
            heading.background_color,
            Some(Color::Rgb {
                r: 54,
                g: 65,
                b: 83
            })
        );
        let style = heading.text_style().unwrap();
        assert_eq!(
            style.color,
            Some(Color::Rgb {
                r: 237,
                g: 240,
                b: 245
            })
        );
        assert_eq!(style.weight, Weight::Bold);
        let rule = canvas.cell(0, 1).unwrap().text_style().unwrap();
        assert_eq!(
            rule.color,
            Some(Color::Rgb {
                r: 119,
                g: 200,
                b: 187
            })
        );
    }
}

#[test]
fn disabled_action_remains_readable_on_the_actual_raised_footer() {
    let canvas = element! {
        View(width:20) {
            DialogFooter {
                DialogAction(label:"Select",enabled:false,has_focus:true)
            }
        }
    }
    .render(None);
    assert_eq!(canvas.to_string().trim_end(), "Select");
    for column in 0..6 {
        let cell = canvas.cell(column, 0).unwrap();
        assert_eq!(
            cell.background_color,
            Some(Color::Rgb {
                r: 48,
                g: 54,
                b: 65
            })
        );
        let style = cell.text_style().unwrap();
        assert_eq!(
            style.color,
            Some(Color::Rgb {
                r: 166,
                g: 175,
                b: 189
            })
        );
        assert_eq!(style.weight, Weight::Normal);
        assert!(
            !style.underline,
            "disabled footer actions cannot advertise focus"
        );
    }
}
