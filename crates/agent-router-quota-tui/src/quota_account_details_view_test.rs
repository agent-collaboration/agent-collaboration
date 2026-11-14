#[test]
fn login_preview_quotes_the_captured_label_and_uses_its_provider() {
    let mut row = crate::quota_browse_presentation_test::quota_two_account_view_model()
        .rows
        .remove(0);
    row.account = "O'Neil Research $literal".to_owned();
    row.provider = codex_router_core::provider::Provider::Claude;
    let target = crate::AccountActionTarget::from_row(&row, 0).unwrap();
    assert_eq!(
        super::login_command_preview(&target),
        "codex-router account login --provider claude --label 'O'\\''Neil Research $literal'"
    );
}
