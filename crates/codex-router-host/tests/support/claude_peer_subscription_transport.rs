//! Publish the real peer registry and observe its authenticated socket frames.
use super::*;

fn publish_peer_registry_record(registry_directory: &Path, socket_path: &Path) -> ProofResult<u32> {
    let process_id = std::process::id();
    std::fs::write(
        registry_directory.join(format!("{process_id}.json")),
        json!({
            "pid": process_id,
            "sessionId": SESSION_ID,
            "status": "idle",
            "peerProtocol": 1,
            "messagingSocketPath": socket_path,
        })
        .to_string(),
    )?;
    let socket_text = socket_path
        .to_str()
        .ok_or_else(|| std::io::Error::other("peer socket path is not UTF-8"))?;
    let digest = Sha256::digest(socket_text.as_bytes());
    let key_path = registry_directory.join(format!("{process_id}.{digest:x}.key"));
    std::fs::write(&key_path, json!({"peerToken": PEER_TOKEN}).to_string())?;
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    Ok(process_id)
}
