//! Native adapter qualification is separate from compiled UI observations.
use super::isolated_fixture_test::{TestResult, ensure};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
mod capture_tools;
pub(super) use capture_tools::{permanent_adapter_source, verified_node_executable};
mod native_adapter_pty_test;
pub(super) fn digest_file(path: &Path) -> TestResult<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

pub(super) fn encode_json_string(value: &str) -> String {
    let mut encoded = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            control if control <= '\u{1f}' => {
                encoded.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

fn run_actual_fixture_reader(
    record: &str,
    check: &str,
    arguments: &[&str],
) -> TestResult<std::process::Output> {
    use std::io::Write as _;
    let (node, _) = verified_node_executable()?;
    let reader = permanent_adapter_source("fixture-child.ts")?;
    let mut child = Command::new(node)
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{ pathToFileURL }} from 'node:url';\nconst [reader, ...args] = process.argv.slice(1);\nconst {{ readFixtureCommand }} = await import(pathToFileURL(reader).href);\nconst record = readFixtureCommand('/dev/stdin');\n{check}\n"
        ))
        .arg(reader)
        .args(arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("reader stdin missing")?
        .write_all(record.as_bytes())?;
    Ok(child.wait_with_output()?)
}

#[test]
fn fixture_command_strings_round_trip_through_the_actual_typescript_reader() -> TestResult<()> {
    let all_controls: String = (0_u8..=31).map(char::from).collect();
    let old_reader =
        run_actual_fixture_reader(&format!("{{\"value\":{all_controls:?}}}"), "", &[])?;
    ensure(
        !old_reader.status.success()
            && String::from_utf8_lossy(&old_reader.stderr).contains("SyntaxError"),
        "old Debug string formatting must fail for JSON control-character escaping",
    )?;
    for expected in [
        "quote\" and backslash\\",
        "Unicode 🦁",
        all_controls.as_str(),
    ] {
        let expected_hex = expected
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ensure(
            run_actual_fixture_reader(
                &format!("{{\"value\":{}}}\n", encode_json_string(expected)),
                "if (Buffer.from(record['value'] ?? '', 'utf8').toString('hex') !== args[0]) process.exit(1);",
                &[&expected_hex],
            )?
            .status
            .success(),
            "fixture JSON must round trip through the actual TypeScript reader",
        )?;
    }
    Ok(())
}
