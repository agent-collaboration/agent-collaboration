//! Opt-in CLI creation and load against the isolated debug Host.
use crate::proof_context::{ProofContext, ProofResult};
use collaboration_client::protocol::SessionRef;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{path::PathBuf, time::Duration};

pub(super) struct ConfigHashGuard {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl ConfigHashGuard {
    pub(super) fn capture() -> ProofResult<Self> {
        let root = PathBuf::from(
            std::env::var_os("CODEX_AUTOMATION_PROOF_ROOT")
                .ok_or("CODEX_AUTOMATION_PROOF_ROOT missing")?,
        );
        let marker: Value =
            serde_json::from_slice(&std::fs::read(root.join("debug-host-context.json"))?)?;
        let owner_home = match marker.get("kind").and_then(Value::as_str) {
            Some("isolatedDeliveryMatrix") => PathBuf::from(
                marker
                    .get("ownerHome")
                    .and_then(Value::as_str)
                    .ok_or("Isolated matrix marker omitted owner home")?,
            ),
            Some("debugHostPrepared") => PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or("Normal HOME missing for the documented debug Host context")?,
            ),
            _ => return Err(
                "Config hash capture requires a supported isolated matrix or debug Host context"
                    .into(),
            ),
        };
        let files = [
            owner_home.join(".codex/config.toml"),
            owner_home.join(".claude/settings.json"),
        ]
        .into_iter()
        .map(|path| {
            let expected = match std::fs::read(&path) {
                Ok(bytes) => Some(Sha256::digest(bytes).to_vec()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            Ok((path, expected))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
        Ok(Self { files })
    }
    pub(super) fn verify(&self) -> ProofResult<()> {
        for (path, expected) in &self.files {
            let observed = match std::fs::read(path) {
                Ok(bytes) => Some(Sha256::digest(bytes).to_vec()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            if &observed != expected {
                return Err(
                    "Owner Codex or Claude settings changed during delivery matrix; stop and report without restoration".into(),
                );
            }
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires a fresh owned isolated CLI Host with scripted provider fixture"]
async fn codex_acp_cli_create_then_prompt_load_route() -> ProofResult<()> {
    let config_guard = ConfigHashGuard::capture()?;
    let mut proof = ProofContext::connect().await?;
    let creator = proof.start_thread("Codex ACP load-route creator").await?;
    let target = cli_codex_create_then_prompt_load_route(&proof, &creator).await?;
    proof.record(
        "codexAcpLoadRoute",
        json!({"target":target,"status":"passedLoadRoute"}),
    )?;
    config_guard.verify()?;
    Ok(())
}

#[expect(
    clippy::indexing_slicing,
    reason = "The fixture protocol and assertions use established positions in their prepared inputs."
)]
async fn cli_codex_create_then_prompt_load_route(
    proof: &ProofContext,
    creator: &SessionRef,
) -> ProofResult<SessionRef> {
    let creator_json = serde_json::to_string(creator)?;
    let create = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "conversation",
            "create",
            "--endpoint",
            "codex-local",
            "--model",
            "gpt-5.6-luna",
            "--effort",
            "low",
            "--access",
            "workspace-write",
            "--from",
            &creator_json,
            "--cwd",
        ])
        .arg(&proof.workspace)
        .arg("--service-directory")
        .arg(&proof.service_directory)
        .arg("--json")
        .output()
        .await?;
    if !create.status.success() {
        return Err("CLI conversation create failed in Codex ACP matrix cell".into());
    }
    let created = String::from_utf8(create.stdout)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line["kind"] == "created")
        .ok_or("CLI conversation create did not return a target")?;
    let target: SessionRef = serde_json::from_value(created["target"].clone())?;
    let target_json = serde_json::to_string(&target)?;
    let prompt = tokio::time::timeout(
        Duration::from_secs(35),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "conversation",
                "prompt",
                "--to",
                &target_json,
                "--from",
                &creator_json,
                "--cwd",
            ])
            .arg(&proof.workspace)
            .args([
                "--text",
                "Reply briefly without tools.",
                "--timeout-seconds",
                "15",
                "--service-directory",
            ])
            .arg(&proof.service_directory)
            .arg("--json")
            .output(),
    )
    .await??;
    let records = String::from_utf8(prompt.stdout)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Err("CLI conversation prompt returned no result".into());
    }
    if !prompt.status.success()
        && records
            .last()
            .and_then(|record| record.pointer("/error/stage"))
            .and_then(Value::as_str)
            != Some("prompt")
    {
        return Err("Codex ACP conversation failed before prompt dispatch".into());
    }
    // The isolated matrix home has no model authentication. Prompt settlement
    // is a separate live cell; here only the route's load admission is proved.
    Ok(target)
}
