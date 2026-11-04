use super::*;
use runtime_primitives::SyncTime;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_700_000_000_000)
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The owner credential must become a sensitive header without retaining the handoff file."
)]
fn owner_handoff_is_consumed_into_a_sensitive_assertion_header() -> TestResult {
    // Arrange the existing credential format, expiring after the injected clock.
    let area = tempfile::tempdir()?;
    let path = area.path().join("owner-credential");
    CentralCredential::Owner {
        endpoint: "https://cutover.invalid".to_owned(),
        assertion: "fixture.owner.assertion".to_owned(),
        expires_at: "2024-01-01T00:00:00Z".to_owned(),
    }
    .write_private_file(&path)?;
    // Act at the credential-to-HTTP boundary using a deterministic clock.
    let headers =
        headers_from_private_file(&Url::parse("https://cutover.invalid")?, &path, &FixedClock)?;
    // Assert exact credential routing, sensitivity and consumed custody.
    let header = headers
        .get("Cf-Access-Jwt-Assertion")
        .ok_or("owner assertion absent")?;
    assert_eq!(header.to_str()?, "fixture.owner.assertion");
    assert!(header.is_sensitive());
    assert!(!headers.contains_key("cf-access-token"));
    assert!(!path.exists());
    Ok(())
}

fn owner_file(
    area: &tempfile::TempDir,
    endpoint: &str,
    expires_at: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = area.path().join("credential");
    CentralCredential::Owner {
        endpoint: endpoint.to_owned(),
        assertion: "fixture.owner.assertion".to_owned(),
        expires_at: expires_at.to_owned(),
    }
    .write_private_file(&path)?;
    Ok(path)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The same owner assertion cannot be sent to a different origin."
)]
fn an_owner_credential_for_another_origin_is_refused() -> TestResult {
    // Arrange a valid private owner handoff for a different origin.
    let area = tempfile::tempdir()?;
    let path = owner_file(&area, "https://another.invalid", "2024-01-01T00:00:00Z")?;
    // Act and assert no HTTP credentials are produced for the wrong origin.
    assert!(matches!(
        headers_from_private_file(&Url::parse("https://cutover.invalid")?, &path, &FixedClock),
        Err(OwnerPublicationAccessError::WrongOrigin)
    ));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Expiry is checked against the injected clock, with equality already expired."
)]
fn an_owner_credential_expiring_at_the_clock_is_refused() -> TestResult {
    // Arrange expiry exactly at FixedClock's millisecond, not the wall clock.
    let area = tempfile::tempdir()?;
    let path = owner_file(&area, "https://cutover.invalid", "2023-11-14T22:13:20Z")?;
    // Act and assert the boundary is exclusive.
    assert!(matches!(
        headers_from_private_file(&Url::parse("https://cutover.invalid")?, &path, &FixedClock),
        Err(OwnerPublicationAccessError::Expired)
    ));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Machine enrollment credentials do not authorize owner publication."
)]
fn an_enrollment_handoff_cannot_be_used_as_an_owner_credential() -> TestResult {
    // Arrange the real enrollment variant in the existing private handoff format.
    let area = tempfile::tempdir()?;
    let path = area.path().join("enrollment");
    CentralCredential::Enrollment {
        endpoint: "https://cutover.invalid".to_owned(),
        owner_assertion: "fixture.owner.assertion".to_owned(),
        owner_expires_at: "2024-01-01T00:00:00Z".to_owned(),
        client_id: "fixture-client".to_owned(),
        assertion: "fixture.machine.assertion".to_owned(),
        expires_at: "2024-01-01T00:00:00Z".to_owned(),
    }
    .write_private_file(&path)?;
    // Act and assert no credential-class fallback occurs.
    assert!(matches!(
        headers_from_private_file(&Url::parse("https://cutover.invalid")?, &path, &FixedClock),
        Err(OwnerPublicationAccessError::WrongKind)
    ));
    Ok(())
}
