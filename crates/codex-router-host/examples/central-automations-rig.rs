//! Verification-only, process-separated local central and Router Host apparatus.
//! No model input is sent. See --help and the central-automations verification journey.
#[path = "central_automations_rig/central_owner.rs"]
mod central_owner;
#[path = "central_automations_rig/host_owner.rs"]
mod host_owner;
#[path = "central_automations_rig/rig_inputs.rs"]
mod rig_inputs;

use rig_inputs::{RigInputs, RigResult};

#[tokio::main]
async fn main() -> RigResult {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments == ["--help"] {
        const USAGE_TEXT: &str = "central-automations-rig central|host-a|host-b PLAN.json\n\
             PLAN: repository, directory (existing private child of repository/tmp),\n\
             optional claude_acp (absolute qualified executable).\n\
             Start central first; its ready line follows two real enrollments.\n\
             Then start each Host in its own process. No proxy or model is started here.\n\
             Claude inputs, when separately driven, must use claude-haiku-5-5 only.\n\
             Central stdin: one JSON command per line: {\"kind\":\"stop\"} or\n\
             {\"kind\":\"control\",\"request\":<RuntimeControlRequest>}.\n\
             EOF, SIGTERM or Ctrl-C drains the exact owned runtime.\n\
             Readiness is apparatus setup, never a central-automations V-row pass.";
        println!("{USAGE_TEXT}");
        return Ok(());
    }
    if !cfg!(debug_assertions) {
        return Err("verification apparatus requires a debug build".into());
    }
    let [mode, plan] = arguments.as_slice() else {
        return Err("expected mode and plan; see --help".into());
    };
    let inputs = RigInputs::read(std::path::Path::new(plan))?;
    match mode.as_str() {
        "central" => central_owner::run(&inputs).await,
        "host-a" => host_owner::run(&inputs, "a").await,
        "host-b" => host_owner::run(&inputs, "b").await,
        _ => Err("unknown apparatus mode; see --help".into()),
    }
}
