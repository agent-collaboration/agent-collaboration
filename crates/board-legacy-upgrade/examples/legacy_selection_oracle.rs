//! Independent legacy inbox selection from sealed SQLite copies.
//! The example follows `ffced1c1`'s inbox predicate and never reads importer output.
#![deny(clippy::too_many_lines)]
#![cfg_attr(test, allow(clippy::panic_in_result_fn))]

#[path = "legacy_selection_oracle/oracle_arguments.rs"]
mod oracle_arguments;
#[path = "legacy_selection_oracle/oracle_errors.rs"]
mod oracle_errors;
#[path = "legacy_selection_oracle/oracle_hashes.rs"]
mod oracle_hashes;
#[path = "legacy_selection_oracle/oracle_receipt.rs"]
mod oracle_receipt;
#[path = "legacy_selection_oracle/post_selection.rs"]
mod post_selection;
#[path = "legacy_selection_oracle/sealed_source.rs"]
mod sealed_source;

#[cfg(test)]
#[path = "legacy_selection_oracle/oracle_tests.rs"]
mod oracle_tests;

use clap::Parser;
use oracle_arguments::OracleArguments;
use oracle_errors::OracleError;
use oracle_receipt::{
    OracleStatus, build_cli_status, build_legacy_oracle_receipt, hash_running_oracle_binary,
    validate_owner_private_output_path, write_new_private_receipt,
};
use sealed_source::read_sealed_copy;
use std::{
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    match execute_legacy_selection_oracle(OracleArguments::parse()) {
        Ok(status) => match serde_json::to_string(&status) {
            Ok(status_json) => match writeln!(io::stdout().lock(), "{status_json}") {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            },
            Err(error) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "legacy selection oracle failed: {error}"
                );
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "legacy selection oracle failed: {error}"
            );
            ExitCode::FAILURE
        }
    }
}

fn execute_legacy_selection_oracle(
    arguments: OracleArguments,
) -> Result<OracleStatus, OracleError> {
    let output_path = validate_owner_private_output_path(&arguments.output)?;
    let source = read_sealed_copy(&arguments.legacy_copy, &arguments.copy_root)?;
    let receipt = build_legacy_oracle_receipt(&source, hash_running_oracle_binary()?)?;
    let status = build_cli_status(&receipt)?;
    write_new_private_receipt(output_path, &receipt)?;
    Ok(status)
}
