//! Native-capabilities verifier: drives journeys J1-J4 on a declared candidate the
//! Lead started through the ordinary `codex-router serve`/`host` lifetimes, and reads
//! the raw evidence back through independent oracles. Usage: `--help`.
#[path = "provider_native_capability_verification/bounded_process.rs"]
mod bounded_process;
#[path = "provider_native_capability_verification/catalogue_oracle.rs"]
mod catalogue_oracle;
#[path = "provider_native_capability_verification/command_options.rs"]
mod command_options;
#[path = "provider_native_capability_verification/command_reload_probe.rs"]
mod command_reload_probe;
#[path = "provider_native_capability_verification/conversation_oracle.rs"]
mod conversation_oracle;
#[path = "provider_native_capability_verification/global_claude_binding.rs"]
mod global_claude_binding;
#[path = "provider_native_capability_verification/installed_client_terminal.rs"]
mod installed_client_terminal;
#[path = "provider_native_capability_verification/journey_run.rs"]
mod journey_run;
#[path = "provider_native_capability_verification/metadata_oracle.rs"]
mod metadata_oracle;
#[path = "provider_native_capability_verification/native_exports.rs"]
mod native_exports;
#[path = "provider_native_capability_verification/native_file_facts.rs"]
mod native_file_facts;
#[path = "provider_native_capability_verification/native_fixture.rs"]
mod native_fixture;
#[path = "provider_native_capability_verification/native_journeys.rs"]
mod native_journeys;
#[path = "provider_native_capability_verification/process_identity.rs"]
mod process_identity;
#[path = "provider_native_capability_verification/router_evidence.rs"]
mod router_evidence;
#[path = "provider_native_capability_verification/router_queue_observation.rs"]
mod router_queue_observation;
#[path = "provider_native_capability_verification/target_qualification.rs"]
mod target_qualification;
#[path = "provider_native_capability_verification/verification_models.rs"]
mod verification_models;

use command_options::{USAGE, VerifierCommand, parse_command};
use journey_run::{Journey, JourneyResult, save_result, user_home};
use native_fixture::{fixture_source, materialize_fixture, write_private};
use process_identity::{
    ProcessIdentity, observe_process_table, own_real_uid, owned_closure, remaining_owned,
};
use std::{collections::BTreeMap, fs, path::Path, process::ExitCode};
use verification_models::NativeCapabilitiesPlan;

/// The repository holding the permanent fixture templates.
fn repository_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read_plan(path: &Path) -> Result<NativeCapabilitiesPlan, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("plan {} unreadable: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("plan does not decode: {error}"))
}

/// Record the owned closure of a Lead-started process; observation only.
fn capture_processes(root_pid: u32, output: &Path) -> Result<String, String> {
    let owned = owned_closure(
        root_pid,
        own_real_uid()?,
        &BTreeMap::new(),
        &observe_process_table()?,
    )?;
    if !owned.contains_key(&root_pid) {
        return Err(format!("process {root_pid} is not running"));
    }
    let json = serde_json::to_string_pretty(&owned).map_err(|error| error.to_string())?;
    write_private(output, json.as_bytes())?;
    Ok(json)
}

/// Recorded processes still present unchanged; a disappeared or reused PID is gone.
fn check_processes(inventory: &Path) -> Result<String, String> {
    let known: BTreeMap<u32, ProcessIdentity> = serde_json::from_slice(
        &fs::read(inventory).map_err(|error| format!("inventory unreadable: {error}"))?,
    )
    .map_err(|error| format!("inventory does not decode: {error}"))?;
    let remaining = remaining_owned(&known, &observe_process_table()?);
    let json = serde_json::to_string_pretty(&remaining).map_err(|error| error.to_string())?;
    if remaining.is_empty() {
        Ok(json)
    } else {
        Err(format!(
            "recorded processes remain; none were signalled:\n{json}"
        ))
    }
}

async fn run_journey(
    plan: &Path,
    output: &Path,
    run: impl AsyncFnOnce(&Journey) -> Result<JourneyResult, String>,
) -> Result<String, String> {
    let plan = read_plan(plan)?;
    let outcome = async {
        let journey = Journey::open(plan, output.to_owned(), user_home()?).await?;
        let result = run(&journey).await?;
        save_result(output, &result)
    }
    .await;
    if let Err(error) = &outcome
        && output.is_dir()
    {
        let _ = write_private(&output.join("failure.txt"), format!("{error}\n").as_bytes());
    }
    outcome
}

async fn run_command(command: VerifierCommand) -> Result<String, String> {
    match command {
        VerifierCommand::Help => Ok(USAGE.to_owned()),
        VerifierCommand::Materialize {
            workspace,
            empty_workspace,
            record,
        } => {
            let fixture = materialize_fixture(
                &fixture_source(repository_root()),
                &workspace,
                &empty_workspace,
                &record,
                &user_home()?,
            )?;
            serde_json::to_string_pretty(&fixture).map_err(|error| error.to_string())
        }
        VerifierCommand::Catalogue { plan, output } => {
            run_journey(&plan, &output, async |journey| {
                native_journeys::catalogue_journey(journey).await
            })
            .await
        }
        VerifierCommand::Select {
            plan,
            output,
            case,
            arguments,
        } => {
            run_journey(&plan, &output, async |journey| {
                native_journeys::selection_journey(journey, case, &arguments).await
            })
            .await
        }
        VerifierCommand::Commands {
            plan,
            output,
            arguments,
            reload_probe,
        } => {
            run_journey(&plan, &output, async |journey| {
                native_journeys::commands_journey(journey, &arguments, reload_probe).await
            })
            .await
        }
        VerifierCommand::History {
            plan,
            output,
            arguments,
            unloaded_resume,
            ordinary_live_capture,
        } => {
            run_journey(&plan, &output, async |journey| {
                native_journeys::history_journey(
                    journey,
                    &arguments,
                    unloaded_resume,
                    &ordinary_live_capture,
                )
                .await
            })
            .await
        }
        VerifierCommand::CaptureProcesses { root_pid, output } => {
            capture_processes(root_pid, &output)
        }
        VerifierCommand::CheckProcesses { inventory } => check_processes(&inventory),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match parse_command(&words) {
        Ok(command) => run_command(command).await,
        Err(error) => Err(format!("{error}\n\n{USAGE}")),
    };
    match outcome {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "provider_native_capability_verification/apparatus_tests.rs"]
mod apparatus_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/conversation_oracle_tests.rs"]
mod conversation_oracle_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/driver_tests.rs"]
mod driver_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/metadata_oracle_tests.rs"]
mod metadata_oracle_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/oracle_test_support.rs"]
mod oracle_test_support;
#[cfg(test)]
#[path = "provider_native_capability_verification/ordinary_completion_tests.rs"]
mod ordinary_completion_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/plugin_catalogue_tests.rs"]
mod plugin_catalogue_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/qualification_tests.rs"]
mod qualification_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/skill_catalogue_tests.rs"]
mod skill_catalogue_tests;
#[cfg(test)]
#[path = "provider_native_capability_verification/skill_source_membership_tests.rs"]
mod skill_source_membership_tests;
