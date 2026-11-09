use super::read_installation_peer;
use std::{io, os::unix::fs::PermissionsExt};

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert no identity creation or replacement."
)]
fn reading_an_installation_peer_never_creates_or_replaces_identity() -> io::Result<()> {
    // Arrange an existing private identity directory with no installation peer.
    let area = tempfile::tempdir()?;
    std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
    // Act through the owning public non-creating reader.
    let absent = read_installation_peer(area.path())?;
    // Assert no generated identity or lock artifact, then exact existing-byte preservation.
    assert_eq!(absent, None);
    assert_eq!(std::fs::read_dir(area.path())?.count(), 0);
    let peer = fleet_protocol::PeerId::new(uuid::Uuid::now_v7()).map_err(io::Error::other)?;
    let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"peerId":peer}))?;
    let path = area.path().join("installation-peer.json");
    std::fs::write(&path, &bytes)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    assert_eq!(read_installation_peer(area.path())?, Some(peer));
    assert_eq!(std::fs::read(&path)?, bytes);
    std::fs::write(&path, b"invalid")?;
    assert!(read_installation_peer(area.path()).is_err());
    assert_eq!(std::fs::read(&path)?, b"invalid");
    Ok(())
}
