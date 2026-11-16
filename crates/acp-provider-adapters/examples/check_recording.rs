//! `cargo run -p acp-provider-adapters --features test-support --example check_recording -- <edge> <recording.jsonl> [--out <results.json>]`
//!
//! The one entry point for live recordings (D0 Program Design Y3): checks a recording with
//! [`codex_router_test_support::node_taps::live_recording::check_recording`], holding Router-owned
//! contracts to the same owning checks the part tests use
//! ([`acp_provider_adapters::owned_contract_checks`]), and writes the
//! results the schema-audit report reads (default `<recording>.check.json`). `<edge>` is one of
//! `cli-codex`, `managed-codex`, `acp-agent`, `acp-served-n8`, `acp-served-n9`, `mcp`. A
//! corrupt recording exits 2 and writes nothing.

use std::{path::PathBuf, process::ExitCode};

use acp_provider_adapters::owned_contract_checks::{round_trips, verdicts};
use codex_router_test_support::node_taps::EdgeDefinitions;
use codex_router_test_support::node_taps::OwnedCodecs;
use codex_router_test_support::node_taps::live_recording::{check_recording, parse_edge};

fn run(arguments: &[String]) -> Result<PathBuf, String> {
    let usage = "usage: check_recording <edge> <recording.jsonl> [--out <results.json>]";
    let (Some(edge_name), Some(recording)) = (arguments.first(), arguments.get(1)) else {
        return Err(usage.to_owned());
    };
    let edge = parse_edge(edge_name).ok_or_else(|| format!("unknown edge {edge_name}; {usage}"))?;
    let out = match (arguments.get(2).map(String::as_str), arguments.get(3)) {
        (Some("--out"), Some(path)) => PathBuf::from(path),
        (None, None) => PathBuf::from(format!("{recording}.check.json")),
        _ => return Err(usage.to_owned()),
    };
    let text =
        std::fs::read_to_string(recording).map_err(|error| format!("read {recording}: {error}"))?;
    let definitions = EdgeDefinitions::load().map_err(|error| error.to_string())?;
    let codecs = OwnedCodecs::from_owner_registry(round_trips(), verdicts())?;
    let report = check_recording(&definitions, edge, recording, &text, &codecs)?;
    let body = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    std::fs::write(&out, body).map_err(|error| format!("write {}: {error}", out.display()))?;
    Ok(out)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(out) => {
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("{problem}");
            ExitCode::from(2)
        }
    }
}
