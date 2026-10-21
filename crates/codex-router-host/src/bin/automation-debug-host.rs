//! Normal-dependency entry for the shared acceptance Host implementation.
#[path = "../../examples/automation-debug-host.rs"]
mod acceptance_host;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    acceptance_host::main()
}
