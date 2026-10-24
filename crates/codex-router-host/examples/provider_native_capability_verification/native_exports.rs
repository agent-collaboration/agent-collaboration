//! The bound native provider's own exports and the installed client's own recorder.
use crate::bounded_process::run_bounded;
use crate::native_file_facts::marketplace_manifest;
use crate::native_fixture::write_private;
use crate::verification_models::{NativeMarketplace, NativePluginRow};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const EXPORT_SECONDS: u64 = 60;
const EXPORT_MAX_BYTES: usize = 4 * 1024 * 1024;
const EXPORT_DIAGNOSTIC_MAX_BYTES: usize = 64 * 1024;
/// The installed client's recorder for one drive; past this the drive is not judged.
const RECORDER_MAX_BYTES: u64 = 64 * 1024 * 1024;
const CONFIG_MAX_BYTES: u64 = 16 * 1024 * 1024;

async fn native_json(executable: &Path, arguments: &[&str], cwd: &Path) -> Result<Vec<u8>, String> {
    let mut command = tokio::process::Command::new(executable);
    command.args(arguments).current_dir(cwd);
    let label = format!("native {}", arguments.join(" "));
    let output = run_bounded(
        command,
        &label,
        Duration::from_secs(EXPORT_SECONDS),
        EXPORT_MAX_BYTES,
        EXPORT_DIAGNOSTIC_MAX_BYTES,
    )
    .await?;
    if !output.status.success() {
        return Err(format!("{label} failed; its output was not retained"));
    }
    Ok(output.stdout)
}

/// `claude plugin list --json` from the bound executable, allow-listed fields only.
pub(crate) async fn native_plugin_rows(
    executable: &Path,
    cwd: &Path,
    evidence: &Path,
    label: &str,
) -> Result<Vec<NativePluginRow>, String> {
    let bytes = native_json(executable, &["plugin", "list", "--json"], cwd).await?;
    let rows: Vec<NativePluginRow> =
        serde_json::from_slice(&bytes).map_err(|error| format!("native plugin export: {error}"))?;
    let json = serde_json::to_vec_pretty(&rows).map_err(|error| error.to_string())?;
    write_private(
        &evidence.join(format!("{label}-native-plugins.json")),
        &json,
    )?;
    Ok(rows)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMarketplaceRow {
    name: String,
    install_location: PathBuf,
}

/// `claude plugin marketplace list --json`: each marketplace with its actual manifest
/// file by native resolution, or unobserved when none exists.
pub(crate) async fn native_marketplaces(
    executable: &Path,
    cwd: &Path,
    evidence: &Path,
    label: &str,
) -> Result<Vec<NativeMarketplace>, String> {
    let bytes = native_json(
        executable,
        &["plugin", "marketplace", "list", "--json"],
        cwd,
    )
    .await?;
    let rows: Vec<NativeMarketplaceRow> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("native marketplace export: {error}"))?;
    let marketplaces: Vec<NativeMarketplace> = rows
        .into_iter()
        .map(|row| NativeMarketplace {
            manifest_path: marketplace_manifest(&row.install_location),
            name: row.name,
        })
        .collect();
    let json = serde_json::to_vec_pretty(&marketplaces).map_err(|error| error.to_string())?;
    write_private(
        &evidence.join(format!("{label}-native-marketplaces.json")),
        &json,
    )?;
    Ok(marketplaces)
}

/// Records from the installed client's own CODEX_TUI_RECORD_SESSION log.
pub(crate) fn read_session_recorder(log: &Path) -> Result<Vec<Value>, String> {
    let contents = read_bounded_text(log, RECORDER_MAX_BYTES)
        .map_err(|error| format!("installed client session recorder: {error}"))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("recorder record: {error}")))
        .collect()
}

/// A whole evidence file read within `limit` bytes; a larger file fails, unread.
pub(crate) fn read_bounded_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("{} unreadable: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{} unreadable: {error}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds its {limit}-byte bound", path.display()));
    }
    Ok(bytes)
}

fn read_bounded_text(path: &Path, limit: u64) -> Result<String, String> {
    String::from_utf8(read_bounded_bytes(path, limit)?)
        .map_err(|_| format!("{} is not UTF-8", path.display()))
}

/// Digest of the global Codex config, recorded around each drive (R5); never a failure.
/// `absent` only when the file does not exist; a file that exists but cannot be read is
/// an error, never mistaken for absence.
pub(crate) fn codex_config_digest(home: &Path) -> Result<String, String> {
    let path = home.join(".codex/config.toml");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("absent".to_owned());
        }
        Err(error) => return Err(format!("global Codex config unobservable: {error}")),
        Ok(_) => {}
    }
    let bytes = read_bounded_bytes(&path, CONFIG_MAX_BYTES)
        .map_err(|error| format!("global Codex config unobservable: {error}"))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
