use super::*;

#[test]
fn extra_cells_follow_reference_weights_and_keep_minima_threshold() {
    assert_eq!(group_widths(0), None);
    assert_eq!(group_widths(84), None);
    for (width, expected) in [
        (85, [17, 29, 18, 18]),
        (86, [17, 30, 18, 18]),
        (120, [26, 41, 26, 24]),
        (159, [36, 54, 35, 31]),
        (160, [36, 54, 35, 32]),
    ] {
        let actual = group_widths(width).unwrap();
        assert_eq!(actual, expected, "reference 24:32:22:17 surplus at {width}");
        assert_eq!(
            actual.iter().sum::<usize>() + 3,
            width,
            "three one-cell gaps"
        );
    }
}

#[test]
fn wide_actual_headers_use_literal_distributed_origins() {
    assert!(render_group_headers(84).is_none());
    for width in [159, 160] {
        let canvas = render_group_headers(width).unwrap().render(None);
        assert_eq!(canvas.width(), width);
        let frame = canvas.to_string();
        let heading = frame.lines().next().unwrap();
        for (label, origin) in [
            ("Account", 0),
            ("Quota left", 37),
            ("Weekly pace", 92),
            ("Extra", 128),
        ] {
            let before = heading.split_once(label).unwrap().0;
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(before),
                origin,
                "{width}: {frame}"
            );
        }
    }
}
