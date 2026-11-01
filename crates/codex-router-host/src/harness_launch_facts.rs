//! Host assertions and launch-time repository observations; neither grants session authority.
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessLaunchFailure, HarnessSessionLaunch,
};
use collaboration_protocol::OperationId;
use fleet_identity_protocol::{
    AgentIsolation, IsolationKind, LaunchContext, MachineEnvironment, StartedBy,
};
use runtime_primitives::IdSource;
use tokio::process::Command;

pub(super) async fn declared_host_launch(
    operation: &OperationId,
    declared: &HarnessDeclaredLaunchInput,
    confirmed_machine_environment: &MachineEnvironment,
    ids: &(dyn IdSource + Send + Sync),
) -> Result<HarnessSessionLaunch, HarnessLaunchFailure> {
    let cwd = declared.working_directory.as_str();
    let repository = git_observation(cwd, &["config", "--get", "remote.origin.url"])
        .await
        .map(|value| value.try_into())
        .transpose()
        .map_err(|_| HarnessLaunchFailure::Unavailable)?;
    let commit = git_observation(cwd, &["rev-parse", "--verify", "HEAD"])
        .await
        .and_then(|value| value.try_into().ok());
    let model = match &declared.settings {
        HarnessDeclaredSettings::Native { model, .. } => model.clone(),
        HarnessDeclaredSettings::Provider { model, .. } => model
            .as_ref()
            .map(|value| String::from(value.clone()).try_into())
            .transpose()
            .map_err(|_| HarnessLaunchFailure::Unavailable)?,
    };
    Ok(HarnessSessionLaunch {
        operation_id: operation.clone(),
        endpoint: declared.endpoint.clone(),
        project_id: declared.project_id.clone(),
        started_by: StartedBy::RouterOperator {},
        isolation: AgentIsolation {
            kind: IsolationKind::Host,
            instance_id: ids
                .new_id()
                .to_string()
                .try_into()
                .map_err(|_| HarnessLaunchFailure::Unavailable)?,
        },
        launch_context: LaunchContext {
            working_directory: declared.working_directory.clone(),
            repository,
            commit,
            model,
        },
        machine_environment: confirmed_machine_environment.clone(),
    })
}

async fn git_observation(cwd: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim_end_matches(['\r', '\n']).to_owned())
}

#[cfg(test)]
#[path = "harness_launch_facts_tests.rs"]
mod tests;
