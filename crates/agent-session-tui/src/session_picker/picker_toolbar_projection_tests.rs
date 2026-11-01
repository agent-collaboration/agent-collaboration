use super::*;
use crate::session_picker::test_support::picker_request;

#[test]
fn toolbar_shrinks_the_count_before_dropping_the_scope_or_new_button() {
    let model = SessionsPickerModel::new(picker_request(), 120);
    let layout = toolbar_layout;
    let wide = layout(&model, 120);
    assert_eq!(wide.count, "1/3 sessions");
    assert_eq!(wide.machine.as_deref(), Some("This machine"));
    assert!(wide.scope_width > 0 && wide.show_new && wide.show_new_key);
    let switch = layout(&model, 80);
    assert_eq!(switch.count, "1/3");
    assert_eq!(switch.machine, None);
    assert!(switch.scope_width > 0 && switch.show_new);
    let narrow = layout(&model, 48);
    assert_eq!(narrow.count, "1/3");
    assert_eq!(narrow.scope_width, 0);
    assert!(narrow.show_new && !narrow.show_new_key);
    let minimum = layout(&model, 32);
    assert_eq!(minimum.count, "1/3");
    assert_eq!(minimum.scope_width, 0);
    assert!(!minimum.show_new);
    assert_eq!(minimum.filter_minimum, 11);
}
