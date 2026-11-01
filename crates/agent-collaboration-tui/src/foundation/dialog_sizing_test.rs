use super::*;
#[test]
fn default_percentages_clamp_independently_to_actual_available_space() {
    for (available, expected) in [
        ((116, 34), (88, 20)),
        ((72, 28), (57, 16)),
        ((44, 22), (42, 13)),
        ((44, 10), (42, 8)),
        ((20, 4), (20, 4)),
        ((0, 0), (0, 0)),
    ] {
        let ((_, width), (_, height)) =
            DialogPresentation::default().resolve(available.0, available.1);
        assert_eq!((width, height), expected);
    }
    assert!(DialogPercentage::new(0).is_none());
    assert!(DialogPercentage::new(101).is_none());
    assert!(DialogAxisBounds::new(9, 8).is_none());
    assert_eq!(
        DialogAxisSize::Content { maximum: Some(88) }.resolve(100),
        (Size::Auto, 88)
    );
}
