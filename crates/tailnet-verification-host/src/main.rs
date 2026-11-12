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
        println!("usage: codex-router-tailnet-verification-host host PLAN.json");
        return Ok(());
    }
    if action == "__launch-host" {
        let binary = arguments
            .next()
            .ok_or(DriverError::Invalid("Host executable required"))?;
        rustix::process::setsid().map_err(std::io::Error::from)?;
        return Err(Command::new(binary).args(arguments).exec().into());
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
