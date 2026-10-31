//! One real Tokio central, two real enrollments, and causal control replies.
use super::rig_inputs::{RigInputs, RigResult};
use board_central_test_support::{RuntimeControlRequest, TestCentral};
use collaboration_runtime::{load_installation_peer, load_service_identity};
use message_board::ActorOrigin;
use serde::Deserialize;
use std::{io::Write, os::unix::fs::DirBuilderExt};
use tokio::io::AsyncBufReadExt;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum CentralCommand {
    Stop,
    Control { request: RuntimeControlRequest },
}

pub async fn run(inputs: &RigInputs) -> RigResult {
    let mut origins = Vec::new();
    for label in ["a", "b"] {
        let directory = inputs.directory.join(label);
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let service = load_service_identity(&directory)?;
        let peer = load_installation_peer(&directory)?;
        origins.push((
            ActorOrigin {
                installation: String::from(service).try_into()?,
                machine: format!("central-automations-{label}").try_into()?,
            },
            peer,
        ));
    }
    let [(origin_a, peer_a), (origin_b, peer_b)] = origins.as_slice() else {
        return Err("two isolated identities required".into());
    };
    let central =
        TestCentral::start_with_peer(&inputs.directory.join("c"), origin_a, *peer_a).await?;
    let second = match central.additional_machine(origin_b, *peer_b).await {
        Ok(second) => second,
        Err(error) => {
            central.shutdown().await?;
            return Err(error);
        }
    };
    let outcome: RigResult = async {
        let enrollment_a = central.write_host_session(&inputs.directory.join("a"))?;
        let enrollment_b = second.write_host_session(&inputs.directory.join("b"))?;
        emit(&serde_json::json!({
            "kind":"centralAutomationsCentralReady", "pid":std::process::id(),
            "origin":central.base_url(), "a":origin_a.installation, "b":origin_b.installation,
            "enrollmentA":enrollment_a, "enrollmentB":enrollment_b,
            "automationsObserved":false
        }))?;
        let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            let command = tokio::select! {
                line = lines.next_line() => line?,
                signal = tokio::signal::ctrl_c() => { signal?; None },
                _signal = terminate.recv() => None,
            };
            let Some(command) = command else { break };
            match serde_json::from_str::<CentralCommand>(&command)? {
                CentralCommand::Stop => break,
                CentralCommand::Control { request } => {
                    let observation = central.runtime_control(request.address, request.command).await?;
                    emit(&serde_json::json!({"kind":"centralControlObserved","observation":observation}))?;
                }
            }
        }
        Ok(())
    }.await;
    let second_stopped = second.shutdown().await;
    let central_stopped = central.shutdown().await;
    second_stopped?;
    central_stopped?;
    outcome?;
    emit(&serde_json::json!({"kind":"centralAutomationsCentralStopped"}))
}

pub fn emit(value: &serde_json::Value) -> RigResult {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
