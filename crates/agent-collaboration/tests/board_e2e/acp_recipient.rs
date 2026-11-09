//! A recipient the Router holds follow mail for and later loads: a session on the Cursor
//! endpoint, served by the journaling fixture ACP provider that a Host started in an armed
//! directory runs. It is absent while its Host's provider has not loaded the session (a
//! fresh Host), and present once `conversation_load` loads it. Every load and prompt the
//! provider receives is appended to a journal in the Host directory.
use super::{fixture::identity, owned_host::BoardHost};
use codex_router_test_support::FixtureResult;
use collaboration_protocol::SessionRef;
use serde_json::{Value, json};
use std::{
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
};
use tokio::{io::AsyncReadExt, net::UnixListener, sync::watch, task::JoinHandle};

pub struct AcpRecipient {
    pub target: SessionRef,
    journal: PathBuf,
    journal_updates: watch::Receiver<u64>,
    journal_notices: JoinHandle<()>,
}

impl AcpRecipient {
    /// Arms `directory` so every Host started there runs the journaling fixture provider,
    /// and names this recipient's session on that Host's Cursor endpoint. The caller
    /// restarts the Host for the provider to start.
    pub fn arm(directory: &Path, service_id: &str, session: &str) -> FixtureResult<Self> {
        let journal = directory.join("fixture-provider-acp.jsonl");
        let journal_signal = directory.join("acp-journal.sock");
        let notices = UnixListener::bind(&journal_signal)?;
        let (revision, journal_updates) = watch::channel(0_u64);
        let launch = json!({
            "executable": node_test_fixture::node_executable()?,
            "arguments": [
                node_test_fixture::fixture_program("provider-loaded-route-agent.ts")
                    .canonicalize()?,
                "journal",
                journal,
                journal_signal,
            ],
        });
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("fixture-provider-acp.json"))
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, serde_json::to_vec(&launch)?.as_slice())
            })?;
        let target = serde_json::from_value(json!({
            "endpoint": {"serviceId": service_id, "endpointId": "cursor-local"},
            "sessionId": session,
        }))?;
        let journal_notices = tokio::spawn(async move {
            while let Ok((mut notice, _)) = notices.accept().await {
                let mut observed = [0_u8; 8];
                if notice.read_exact(&mut observed).await.is_ok() && &observed == b"observed" {
                    revision.send_modify(|value| *value = value.wrapping_add(1));
                }
            }
        });
        Ok(Self {
            target,
            journal,
            journal_updates,
            journal_notices,
        })
    }

    /// Makes the recipient present: the Host's provider loads its session. A load
    /// completion is the signal on which held mail for it is checked again.
    pub async fn load(&self, host: &BoardHost) -> FixtureResult {
        host.mcp(
            "conversation_load",
            json!({
                "operationId": identity(),
                "target": self.target,
                "workingDirectory": std::env::temp_dir(),
                "requestedBy": {"humanId": "lead"},
                "access": "write-restricted",
            }),
        )
        .await?;
        Ok(())
    }

    /// Makes the recipient present so that its load is what releases its held mail. First
    /// the reader finishes a pass with the recipient still absent (a subscriptions read
    /// answers from a completed reconcile pass), so its held mail is checked and stays
    /// held; then the provider loads the session. The caller does nothing else that checks
    /// held mail before it observes the release.
    pub async fn come_back(&self, host: &BoardHost) -> FixtureResult {
        host.mcp(
            "thread_subscriptions",
            json!({"actor":{"kind":"session","session":self.target}}),
        )
        .await?;
        self.load(host).await
    }

    /// The text of every prompt the provider received for this session, in order.
    pub fn prompts(&self) -> FixtureResult<Vec<String>> {
        let journal = match std::fs::read_to_string(&self.journal) {
            Ok(journal) => journal,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut prompts = Vec::new();
        for line in journal.lines().filter(|line| !line.is_empty()) {
            let request: Value = serde_json::from_str(line)?;
            if request.get("method") != Some(&json!("session/prompt"))
                || request.pointer("/params/sessionId") != Some(&json!(self.target.session_id))
            {
                continue;
            }
            let text = request
                .pointer("/params/prompt")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            prompts.push(text);
        }
        Ok(prompts)
    }

    /// Waits for a prompt containing `needle`, and returns its text.
    pub async fn expect_text(&self, needle: &str) -> FixtureResult<String> {
        let mut updates = self.journal_updates.clone();
        loop {
            updates.borrow_and_update();
            if let Some(text) = self
                .prompts()?
                .into_iter()
                .find(|text| text.contains(needle))
            {
                return Ok(text);
            }
            updates.changed().await?;
        }
    }

    pub async fn expect_marker(&self, marker: &str) -> FixtureResult {
        self.expect_text(marker).await.map(|_| ())
    }

    /// Prompts received so far that contain `needle`.
    pub fn count_matching(&self, needle: &str) -> FixtureResult<u64> {
        Ok(u64::try_from(
            self.prompts()?
                .iter()
                .filter(|text| text.contains(needle))
                .count(),
        )?)
    }

    /// Every prompt received so far. The provider journals a prompt before it answers, so
    /// a prompt the Router sent and saw answered is already counted.
    pub fn count(&self) -> FixtureResult<u64> {
        Ok(u64::try_from(self.prompts()?.len())?)
    }
}

impl Drop for AcpRecipient {
    fn drop(&mut self) {
        self.journal_notices.abort();
    }
}

#[cfg(test)]
#[path = "acp_recipient_test.rs"]
mod tests;
