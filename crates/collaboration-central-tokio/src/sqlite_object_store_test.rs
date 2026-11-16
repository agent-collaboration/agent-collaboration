use super::statement_is_rejected;

#[test]
fn only_allowlisted_rejected_statement_codes_are_charged_to_an_item() {
    for code in [19, 20, 531, 1043, 1811, 2323, 2579, 2835, 3091] {
        assert!(statement_is_rejected(Some(&code.to_string())), "{code}");
    }
    for code in [5, 6, 7, 10, 13, 1, 778, 999_999, 19 + 256 * 99] {
        assert!(!statement_is_rejected(Some(&code.to_string())), "{code}");
    }
    assert!(!statement_is_rejected(None));
    assert!(!statement_is_rejected(Some("unrecognized")));
}
