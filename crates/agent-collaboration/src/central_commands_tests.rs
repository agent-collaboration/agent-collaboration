//! Parse real command arguments and build the production request without network I/O.
#![allow(clippy::panic_in_result_fn)]
use super::*;
use serde_json::{Value, json};
const PROJECT: &str = "019f0000-0000-7000-8000-000000005881";
const INSTALLATION: &str = "550e8400-e29b-41d4-a716-446655440583";
fn parse(arguments: &[&str]) -> Result<CentralArguments, clap::Error> {
    CentralArguments::try_parse_from(std::iter::once("central").chain(arguments.iter().copied()))
}
fn request(arguments: &[&str]) -> Result<reqwest::Request, Box<dyn std::error::Error>> {
    let arguments = parse(arguments)?;
    let operation = prepare(arguments.command)?;
    Ok(CentralAdminClient::new(
        arguments
            .central
            .as_deref()
            .ok_or("--central is required")?,
        "not-a-live-credential",
    )?
    .prepare(&operation)?)
}
#[test]
fn central_is_explicit_and_invalid_page_limits_fail_before_owner_sign_in() {
    assert!(parse(&["copies"]).unwrap().central.is_none());
    assert!(
        parse(&["project", "takeover", "preview", PROJECT])
            .unwrap()
            .central
            .is_none()
    );
    for limit in ["0", "26", "100"] {
        let args = parse(&[
            "--central",
            "https://central.example",
            "migrate",
            "--limit",
            limit,
        ])
        .unwrap();
        assert!(prepare(args.command).is_err());
    }
}
#[test]
fn copy_and_page_commands_build_one_exact_request_each() -> Result<(), Box<dyn std::error::Error>> {
    for (args, method, suffix) in [
        (
            vec!["copies", "--after", PROJECT, "--limit", "7"],
            "GET",
            "/admin/v1/copies".to_owned(),
        ),
        (
            vec!["migrate", "--after", PROJECT, "--limit", "25"],
            "POST",
            "/admin/v1/copies/migrate".to_owned(),
        ),
        (
            vec!["copy", "health", PROJECT],
            "GET",
            format!("/admin/v1/copies/{PROJECT}/health"),
        ),
        (
            vec!["copy", "repair", PROJECT],
            "POST",
            format!("/admin/v1/copies/{PROJECT}/repair"),
        ),
        (
            vec!["project", "takeover", "preview", PROJECT],
            "POST",
            format!("/admin/v1/projects/{PROJECT}/takeover/preview"),
        ),
    ] {
        let mut arguments = vec!["--central", "https://central.example"];
        arguments.extend(args);
        let actual = request(&arguments)?;
        assert_eq!(actual.method(), method);
        assert_eq!(actual.url().path(), suffix);
        let access = actual
            .headers()
            .get("cf-access-token")
            .ok_or("owner Access token absent")?;
        assert!(access.is_sensitive());
        assert_eq!(access.to_str()?, "not-a-live-credential");
        assert!(!actual.headers().contains_key("Cf-Access-Jwt-Assertion"));
        if actual.url().path().ends_with("copies") || actual.url().path().ends_with("migrate") {
            let pairs: std::collections::BTreeMap<_, _> =
                actual.url().query_pairs().into_owned().collect();
            assert_eq!(pairs.get("after").map(String::as_str), Some(PROJECT));
            assert!(pairs.contains_key("limit"));
        }
    }
    Ok(())
}
#[test]
fn move_retire_and_confirmation_preserve_native_ids_positions_and_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let preview = json!({"token":PROJECT,"fence":1,"acked":{"epoch":1,"sequence":7},"holder":"019f0000-0000-7000-8000-000000005583","lastSeen":1900000000000_i64});
    let preview_file = tempfile::NamedTempFile::new()?;
    std::fs::write(preview_file.path(), serde_json::to_vec(&preview)?)?;
    let preview_path = preview_file
        .path()
        .to_str()
        .ok_or("test preview path is not UTF-8")?;
    let transfer = [
        "--transfer-id",
        PROJECT,
        "--to-installation",
        INSTALLATION,
        "--reason",
        "recover holder",
    ];
    let mut confirm = vec![
        "--central",
        "https://central.example",
        "project",
        "takeover",
        "confirm",
        PROJECT,
    ];
    confirm.extend(transfer);
    confirm.extend(["--preview", preview_path]);
    let confirmation = request(&confirm)?;
    let body: Value = serde_json::from_slice(
        confirmation
            .body()
            .and_then(reqwest::Body::as_bytes)
            .ok_or("confirmation body absent")?,
    )?;
    assert_eq!(
        body,
        json!({"transferId":PROJECT,"toInstallation":INSTALLATION,"reason":"recover holder","preview":preview})
    );
    let mut moving = vec![
        "--central",
        "https://central.example",
        "project",
        "move",
        PROJECT,
    ];
    moving.extend(transfer);
    let moving = request(&moving)?;
    assert_eq!(
        moving.url().path(),
        format!("/admin/v1/projects/{PROJECT}/move")
    );
    let retired = request(&[
        "--central",
        "https://central.example",
        "project",
        "retire",
        PROJECT,
        "--request-id",
        PROJECT,
        "--expect-fence",
        "1",
        "--expect-acked",
        r#"{"epoch":1,"sequence":7}"#,
    ])?;
    assert_eq!(
        serde_json::from_slice::<Value>(
            retired
                .body()
                .and_then(reqwest::Body::as_bytes)
                .ok_or("retire body absent")?
        )?,
        json!({"requestId":PROJECT,"expectFence":1,"expectAcked":{"epoch":1,"sequence":7}})
    );
    Ok(())
}
#[test]
fn preview_labels_distinguish_presence_from_renewal_and_warn_about_unsent_work()
-> Result<(), Box<dyn std::error::Error>> {
    let preview: OwnerPreviewSuccess = serde_json::from_value(
        json!({"preview":{"token":PROJECT,"fence":1,"acked":{"epoch":1,"sequence":7},"holder":"019f0000-0000-7000-8000-000000005583","lastSeen":1900000000000_i64},"holderInstallation":INSTALLATION,"lastSeen":1900000060000_i64}),
    )?;
    let output = central_output::preview_text(&preview);
    assert!(output.contains("Last renewal: 1900000000000"), "{output}");
    assert!(output.contains("Last seen: 1900000060000"), "{output}");
    assert!(
        output.contains("Anything the holder had not sent will not be carried over."),
        "{output}"
    );
    Ok(())
}

#[test]
fn unknown_effects_remain_distinct_from_a_known_refusal() {
    let unknown = CentralCommandFailure::Admin(
        collaboration_client::CentralAdminError::OutcomeUnknown("lost answer".into()),
    );
    assert_eq!(unknown.report(), ("outcomeUnknown", 3));
    assert_eq!(
        CentralCommandFailure::Access(collaboration_client::OwnerAccessError::SignInRequired)
            .report(),
        ("rejected", 4)
    );
}
