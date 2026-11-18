//! Prepare private fault input on stopped owners; read custody through the pinned SDK observer.

pub fn sender_binding(
    directory: &std::path::Path,
) -> std::io::Result<collaboration_service::MachineSenderBinding> {
    let service = collaboration_runtime::read_service_identity(directory)?
        .ok_or_else(|| std::io::Error::other("stopped owner has no service identity"))?;
    // The public peer loader creates only when absent. Require the existing file
    // before calling it on this stopped owner so observation never mints an identity.
    std::fs::symlink_metadata(directory.join("installation-peer.json"))?;
    let peer = collaboration_runtime::load_installation_peer(directory)?;
    Ok(collaboration_service::MachineSenderBinding::new(
        String::from(service)
            .try_into()
            .map_err(std::io::Error::other)?,
        peer,
    ))
}
