use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use codex_router_tailnet_verification_host::bounded_command::install_termination_handler;
use codex_router_tailnet_verification_host::driver_error::{DriverError, DriverResult};
use codex_router_tailnet_verification_host::driver_records::read_record;
use codex_router_tailnet_verification_host::host_lifecycle::run_host;

fn run() -> DriverResult<()> {
    let mut arguments = std::env::args().skip(1);
    let action = arguments
        .next()
        .ok_or(DriverError::Invalid("host action and plan required"))?;
    if action == "--help" || action == "-h" {
        println!(
            "usage: codex-router-tailnet-verification-host host PLAN.json\n       codex-router-tailnet-verification-host d1-native-ready PLAN.json\nD1 native readiness sends initialize/initialized/account-read only; no thread, turn, login or model call."
        );
        return Ok(());
    }
    if action == "__launch-host" {
        let binary = arguments
            .next()
            .ok_or(DriverError::Invalid("Host executable required"))?;
        rustix::process::setsid().map_err(std::io::Error::from)?;
        return Err(Command::new(binary).args(arguments).exec().into());
    }
    if action == "d1-native-ready" {
        let path = arguments
            .next()
            .ok_or(DriverError::Invalid("D1 plan required"))?;
        if arguments.next().is_some() {
            return Err(DriverError::Invalid("unexpected D1 argument"));
        }
        install_termination_handler()?;
        return codex_router_tailnet_verification_host::d1_host_lifecycle::run_native_readiness(
            &codex_router_tailnet_verification_host::d1_host_plan::read_d1_plan(Path::new(&path))?,
        );
    }
    if action != "host" {
        return Err(DriverError::Invalid("expected host action"));
    }
    let path = arguments
        .next()
        .ok_or(DriverError::Invalid("Host plan required"))?;
    if arguments.next().is_some() {
        return Err(DriverError::Invalid("unexpected Host argument"));
    }
    install_termination_handler()?;
    run_host(&read_record(Path::new(&path))?)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error}", error.category());
        std::process::exit(1);
    }
}
