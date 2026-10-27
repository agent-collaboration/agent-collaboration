#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions fail these tests while fixture errors propagate through Result."
)]
use codex_router_tailnet_verification_host::process_identity::parse_identity;

#[test]
fn process_fields_preserve_birth_and_executable_spacing() -> Result<(), Box<dyn std::error::Error>>
{
    let identity = parse_identity(
        "    1     0     0     1 Sun Oct  4 13:51:12 2026     /tmp/Example  Program",
        true,
    )?;
    assert_eq!(identity.pid, 1);
    assert_eq!(identity.birth, "Sun Oct 4 13:51:12 2026");
    assert_eq!(identity.executable, "/tmp/Example  Program");
    let nobody = parse_identity(
        "   10     1    -2    10 Sun Oct  4 13:51:12 2026     /tmp/owned-fixture",
        true,
    )?;
    assert_eq!(nobody.uid, -2);
    Ok(())
}
