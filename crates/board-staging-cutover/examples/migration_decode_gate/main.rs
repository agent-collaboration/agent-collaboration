//! Verification-only decode gate between PD7's inventory and migrate.
//!
//!   migration_decode_gate <inventory answer.json>
//!
//! Reads the migration Worker's inventory answer (call_migration.ts inventory
//! --out), checks its candidate hashes to the digest it names, rebuilds every
//! candidate object in a private in-memory database and decodes it with
//! main's real decoders. Prints one JSON report, `{outcome, candidateDigest,
//! runTimeMillis, refusals}`; exit 0 only on `pass`. Migrate takes the passed
//! digest and run time (call_migration.ts migrate --gate) and refuses any
//! other candidate before its first write.

mod candidate_image;
mod gate;
mod main_decoders;
mod object_rebuild;

#[cfg(test)]
mod gate_test;

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: migration_decode_gate <inventory answer.json>");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    let report = gate::run_gate(&text).await;
    match serde_json::to_string(&report) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("cannot write the report: {error}");
            return ExitCode::from(2);
        }
    }
    if report.outcome == gate::Outcome::Pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
