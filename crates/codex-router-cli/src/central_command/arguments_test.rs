use super::*;
#[test]
fn serving_requires_an_explicit_legacy_organization() {
    let missing = CentralCommand::parse(
        ["serve", "--root", "/unused-b2-fixture"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    );
    assert!(matches!(
        missing,
        Err(super::super::CentralCommandError::Arguments(_))
    ));
    let configured = CentralCommand::parse(
        [
            "serve",
            "--root",
            "/unused-b2-fixture",
            "--legacy-organization-id",
            "019f0000-0000-7000-8000-00000000ac01",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    );
    assert!(
        matches!(configured, Ok(CentralCommand::Serve(arguments)) if arguments.legacy_organization_id == "019f0000-0000-7000-8000-00000000ac01")
    );
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertion checks refusal before any state-root effect; fixture errors propagate."
)]
#[tokio::test]
async fn invalid_legacy_organization_refuses_before_creating_state()
-> Result<(), Box<dyn std::error::Error>> {
    let parent = tempfile::tempdir()?;
    let root = parent.path().join("must-not-be-created");
    let command = CentralCommand::parse(vec![
        "serve".into(),
        "--root".into(),
        root.as_os_str().to_owned(),
        "--legacy-organization-id".into(),
        "fleet".into(),
    ])?;
    let result = super::super::run_central_command(command, &mut Vec::new()).await;
    assert!(matches!(
        result,
        Err(super::super::CentralCommandError::InvalidInput(
            "invalid legacy organization UUIDv7"
        ))
    ));
    assert!(!root.exists());
    Ok(())
}
