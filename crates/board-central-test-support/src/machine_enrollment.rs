//! Each Router uses a distinct real enrollment and Access bridge on one central.
use crate::{BOUND, FixtureResult, access_edge::SignedAccessEdge};
use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use collaboration_central_conformance::{access_client_id, owner_assertion};
use fleet_sync_protocol::{PeerId, ReceiverId};
use fleet_sync_session::{Endpoint, PeerCredential};
use message_board::{ActorOrigin, ServiceId};
use sha2::{Digest, Sha256};

pub struct EnrolledTestMachine {
    pub(super) edge: SignedAccessEdge,
    pub(super) receiver: ReceiverId,
    peer: PeerId,
    relay: PeerId,
    credential: String,
    installation: ServiceId,
}

impl EnrolledTestMachine {
    /// Count actual established upstream sessions; no inferred reconnect evidence.
    /// Repeat one real incoming hint on the wire, preserving its actual position.
    pub fn duplicate_next_mail_notice(&self) {
        self.edge.duplicate_next_mail_notice();
    }
    pub fn duplicated_mail_notices(&self) -> usize {
        self.edge.duplicated_mail_notices()
    }
    pub fn session_connections(&self) -> usize {
        self.edge.session_connections()
    }
    pub fn hold_granted_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<crate::LeaseNoticeObservation> {
        self.edge.hold_granted_notice(project, epoch)
    }
    pub fn hold_superseded_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<crate::LeaseNoticeObservation> {
        self.edge.hold_superseded_notice(project, epoch)
    }
    pub(super) async fn enroll(
        upstream: &str,
        origin: &ActorOrigin,
        peer: PeerId,
        receiver: ReceiverId,
        credential: String,
    ) -> FixtureResult<Self> {
        let response: serde_json::Value = reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .post(format!("{upstream}/admin/v1/machines/enroll"))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&serde_json::json!({"kind":"enrollMachine","value":{
                "label":origin.machine, "installation":origin.installation,
                "peer":peer,
                "access_client_id":access_client_id(&credential)?,
                "credential_hash":format!("{:x}", Sha256::digest(credential.as_bytes())),
                "expires_at":"2033-05-18T03:33:20Z"
            }}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if response.get("kind") != Some(&serde_json::json!("machineEnrolled")) {
            return Err("real test central did not enroll the machine".into());
        }
        let value = response
            .get("value")
            .ok_or("machine enrollment omitted its value")?;
        let actual_peer: PeerId = serde_json::from_value(
            value
                .get("peer")
                .cloned()
                .ok_or("machine enrollment omitted its peer")?,
        )?;
        let actual_receiver: ReceiverId = serde_json::from_value(
            value
                .get("authoritativeReceiver")
                .cloned()
                .ok_or("machine enrollment omitted its receiver")?,
        )?;
        let relay: PeerId = serde_json::from_value(
            value
                .get("relayPeer")
                .cloned()
                .ok_or("machine enrollment omitted its relay identity")?,
        )?;
        if actual_peer != peer
            || actual_receiver != receiver
            || relay.to_string() != "019f0000-0000-7000-8000-000000005240"
        {
            return Err(
                "machine enrollment changed supplied identities or deployment receiver/relay"
                    .into(),
            );
        }
        let edge = SignedAccessEdge::start(upstream, &credential).await?;
        Ok(Self {
            edge,
            receiver,
            peer,
            relay,
            credential,
            installation: origin.installation.clone(),
        })
    }

    pub fn session_inputs(&self) -> FixtureResult<BoardSessionInputs> {
        Ok(BoardSessionInputs {
            endpoint: Endpoint::new(&self.edge.endpoint)?,
            access: AccessServiceToken::new("board-fixture-client", "board-fixture-secret")?,
            credential: PeerCredential::new(format!("Bearer {}", self.credential))?,
            peer: self.peer,
            relay: self.relay,
            receiver: self.receiver,
        })
    }

    /// Export only this fixture enrollment for the owned board Host child.
    #[cfg(debug_assertions)]
    pub fn write_host_session(
        &self,
        directory: &std::path::Path,
    ) -> FixtureResult<std::path::PathBuf> {
        use codex_router_core::redaction::SecretString;
        use codex_router_secret_store::{
            SecretStore,
            file_backend::FileSecretStore,
            fleet_secret_key::{FleetSecretKey, FleetSecretKind},
        };
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let generation = message_board::RequestId::generate();
        let access_key = FleetSecretKey::new(
            FleetSecretKind::AccessClientSecret,
            self.installation.as_str(),
            generation.as_str(),
        )?
        .secret_key()?;
        let credential_key = FleetSecretKey::new(
            FleetSecretKind::MachineCredential,
            self.installation.as_str(),
            generation.as_str(),
        )?
        .secret_key()?;
        // This dev-only fixture explicitly declares its isolated debug secret root.
        let secrets = FileSecretStore::initialize_debug_plaintext(&directory.join("secrets"))?;
        secrets.write_secret(&access_key, &SecretString::new("board-fixture-secret"))?;
        secrets.write_secret(
            &credential_key,
            &SecretString::new(format!("Bearer {}", self.credential)),
        )?;
        let path = directory.join("fixture-session.json");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&serde_json::to_vec(&serde_json::json!({
            "endpoint":self.edge.endpoint,
            "access":{"clientId":"board-fixture-client","clientSecretKey":access_key.as_str()},
            "credentialKey":credential_key.as_str(),
            "peer":self.peer,"receiver":self.receiver,"relay":self.relay,
        }))?)?;
        Ok(path)
    }

    #[cfg(not(debug_assertions))]
    pub fn write_host_session(
        &self,
        _directory: &std::path::Path,
    ) -> FixtureResult<std::path::PathBuf> {
        Err("fixture enrollment requires an isolated debug build".into())
    }

    pub async fn shutdown(self) -> FixtureResult {
        self.edge.shutdown().await
    }
}
