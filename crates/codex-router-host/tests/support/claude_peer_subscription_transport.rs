//! Publish the real peer registry and observe its authenticated socket frames.
use super::*;

impl ClaudePeerSubscriptionFixture {
    pub fn lookup_peer(&self) -> ProofResult<PeerSessionLookup> {
        Ok(self.peer_registry.lookup(&self.target.session_id)?)
    }

    pub fn publish_writable_peer(&self) -> ProofResult<(Arc<UnixListener>, PeerSessionRecord)> {
        let listener = Arc::new(UnixListener::bind(&self.socket_path)?);
        let process_id = publish_peer_registry_record(&self.registry_directory, &self.socket_path)?;
        match self.peer_registry.lookup(&self.target.session_id)? {
            PeerSessionLookup::Writable(peer)
                if u32::from(peer.process_id) == process_id
                    && peer.session_id == self.target.session_id
                    && peer.socket_path == self.socket_path =>
            {
                Ok((listener, peer))
            }
            _ => Err(std::io::Error::other(
                "published Claude peer did not become writable in the real registry",
            )
            .into()),
        }
    }

    pub fn receive_one_peer_batch(
        listener: Arc<UnixListener>,
    ) -> JoinHandle<ProofResult<PeerMessageFrames>> {
        tokio::spawn(async move {
            let (stream, _) = tokio::time::timeout(OWNER_EVENT_TIMEOUT, listener.accept())
                .await
                .map_err(|_| std::io::Error::other("Claude peer did not accept the notice"))??;
            let mut lines = BufReader::new(stream).lines();
            let authentication: Value = serde_json::from_str(
                &lines
                    .next_line()
                    .await?
                    .ok_or_else(|| std::io::Error::other("Claude peer auth frame is missing"))?,
            )?;
            let user: Value =
                serde_json::from_str(&lines.next_line().await?.ok_or_else(|| {
                    std::io::Error::other("Claude peer user Batch frame is missing")
                })?)?;
            let extra_frame = lines.next_line().await?;
            Ok(PeerMessageFrames {
                authentication,
                user,
                extra_frame,
            })
        })
    }
}

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
