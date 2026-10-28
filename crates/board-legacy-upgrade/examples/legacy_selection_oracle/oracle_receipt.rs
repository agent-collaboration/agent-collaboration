use crate::{
    oracle_errors::OracleError,
    oracle_hashes::{sha256_file, sha256_post_set},
    post_selection::{
        OraclePostSelection, ReaderCutoffs, SelectionMode, derive_reader_cutoffs, selected_post_ids,
    },
    sealed_source::{LegacyActivityKind, SealedLegacyCopy},
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct LegacyOracleReceipt {
    pub(crate) kind: &'static str,
    pub(crate) product_import_observed: bool,
    pub(crate) source_sha256: String,
    pub(crate) driver_sha256: String,
    pub(crate) activity_count: u64,
    pub(crate) selections: Vec<OraclePostSelection>,
    pub(crate) projects: Vec<LegacyProjectSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct LegacyProjectSummary {
    pub(crate) legacy_topic: String,
    pub(crate) message_creation_activity_count: u64,
    pub(crate) readers: Vec<ReaderCutoffs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OracleStatus {
    pub(crate) kind: &'static str,
    pub(crate) activity_count: u64,
    pub(crate) selection_count: u64,
    pub(crate) product_assertions: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrivateOutputPath(PathBuf);

pub(crate) fn build_legacy_oracle_receipt(
    source: &SealedLegacyCopy,
    driver_sha256: String,
) -> Result<LegacyOracleReceipt, OracleError> {
    let rows = &source.rows;
    let mut selections = Vec::new();
    for reader in &rows.reader_keys {
        for topic in &rows.topic_ids {
            for mode in SelectionMode::ORDERED {
                let post_ids = selected_post_ids(rows, reader, topic, mode);
                selections.push(OraclePostSelection {
                    post_count: count_as_u64(post_ids.len())?,
                    covered_post_set_sha256: sha256_post_set(&post_ids)?,
                    reader: reader.clone(),
                    legacy_topic: topic.clone(),
                    mode,
                    post_ids,
                });
            }
        }
    }

    let projects = rows
        .topic_ids
        .iter()
        .map(|topic| {
            let message_creation_activity_count = rows
                .activity
                .iter()
                .filter(|activity| {
                    activity.topic_id == *topic
                        && activity.message_id.is_some()
                        && matches!(
                            &activity.kind,
                            LegacyActivityKind::MainMessageCreated
                                | LegacyActivityKind::ThreadMessageCreated
                        )
                })
                .count();
            let readers = rows
                .reader_keys
                .iter()
                .map(|reader| derive_reader_cutoffs(rows, reader, topic))
                .collect();
            Ok(LegacyProjectSummary {
                legacy_topic: topic.clone(),
                message_creation_activity_count: count_as_u64(message_creation_activity_count)?,
                readers,
            })
        })
        .collect::<Result<Vec<_>, OracleError>>()?;

    Ok(LegacyOracleReceipt {
        kind: "legacyOracleOnly",
        product_import_observed: false,
        source_sha256: source.source_sha256.clone(),
        driver_sha256,
        activity_count: count_as_u64(rows.activity.len())?,
        selections,
        projects,
    })
}

pub(crate) fn hash_running_oracle_binary() -> Result<String, OracleError> {
    let executable_path = std::env::current_exe()?;
    sha256_file(&executable_path)
}

pub(crate) fn build_cli_status(receipt: &LegacyOracleReceipt) -> Result<OracleStatus, OracleError> {
    Ok(OracleStatus {
        kind: receipt.kind,
        activity_count: receipt.activity_count,
        selection_count: count_as_u64(receipt.selections.len())?,
        product_assertions: 0,
    })
}

pub(crate) fn write_new_private_receipt(
    output_path: PrivateOutputPath,
    receipt: &LegacyOracleReceipt,
) -> Result<(), OracleError> {
    let mut contents = serde_json::to_vec_pretty(receipt)?;
    contents.push(b'\n');

    #[cfg(unix)]
    let mut output_file = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        open_new_output_file(&output_path.0, options)?
    };
    #[cfg(not(unix))]
    let mut output_file = {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        open_new_output_file(&output_path.0, options)?
    };

    let write_result = write_private_file_contents(&mut output_file, &contents);
    if write_result.is_err() {
        drop(output_file);
        let _ = fs::remove_file(&output_path.0);
    }
    write_result
}

pub(crate) fn validate_owner_private_output_path(
    output_path: &Path,
) -> Result<PrivateOutputPath, OracleError> {
    if !output_path.is_absolute() {
        return Err(OracleError::InvalidOutputDirectory);
    }
    let parent_path = output_path
        .parent()
        .ok_or(OracleError::InvalidOutputDirectory)?;
    let file_name = output_path
        .file_name()
        .ok_or(OracleError::InvalidOutputDirectory)?;
    let canonical_parent = parent_path.canonicalize()?;
    let home_path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(OracleError::InvalidOutputDirectory)?;
    let cache_root = home_path.join(".cache").canonicalize()?;
    let parent_metadata = canonical_parent.metadata()?;
    if canonical_parent == cache_root
        || !canonical_parent.starts_with(&cache_root)
        || !parent_metadata.is_dir()
        || !is_private_owner_directory(&parent_metadata)
    {
        return Err(OracleError::InvalidOutputDirectory);
    }
    Ok(PrivateOutputPath(canonical_parent.join(file_name)))
}

#[cfg(unix)]
fn is_private_owner_directory(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == nix::unistd::geteuid().as_raw() && metadata.mode() & 0o777 == 0o700
}

#[cfg(not(unix))]
fn is_private_owner_directory(_metadata: &fs::Metadata) -> bool {
    false
}

fn open_new_output_file(output_path: &Path, options: OpenOptions) -> Result<fs::File, OracleError> {
    match options.open(output_path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(OracleError::OutputAlreadyExists)
        }
        Err(error) => Err(OracleError::Io(error)),
    }
}

fn write_private_file_contents(
    output_file: &mut fs::File,
    contents: &[u8],
) -> Result<(), OracleError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output_file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    output_file.write_all(contents)?;
    output_file.sync_all()?;
    Ok(())
}

fn count_as_u64(count: usize) -> Result<u64, OracleError> {
    u64::try_from(count).map_err(|_| OracleError::CountOutOfRange)
}
