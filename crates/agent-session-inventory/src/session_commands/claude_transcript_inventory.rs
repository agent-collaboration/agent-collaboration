//! Read-only Claude transcript discovery; native live facts have a different owner.
use std::{fs, path::Path};

pub(super) fn list_stored_transcript_records(
    projects_directory: &Path,
) -> Result<Vec<serde_json::Value>, String> {
    let project_directories = match fs::read_dir(projects_directory) {
        Ok(project_directories) => project_directories,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Claude projects directory unavailable: {error}")),
    };
    let mut records = Vec::new();
    for project_entry in project_directories {
        let project_entry =
            project_entry.map_err(|error| format!("Claude project entry unavailable: {error}"))?;
        if !project_entry
            .file_type()
            .map_err(|error| format!("Claude project entry metadata unavailable: {error}"))?
            .is_dir()
        {
            continue;
        }
        let project_name = project_entry.file_name().to_string_lossy().into_owned();
        for transcript_entry in fs::read_dir(project_entry.path())
            .map_err(|error| format!("Claude project transcripts unavailable: {error}"))?
        {
            let transcript_entry = transcript_entry
                .map_err(|error| format!("Claude transcript entry unavailable: {error}"))?;
            if !transcript_entry
                .file_type()
                .map_err(|error| format!("Claude transcript metadata unavailable: {error}"))?
                .is_file()
            {
                continue;
            }
            let transcript_path = transcript_entry.path();
            if transcript_path
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("jsonl")
            {
                continue;
            }
            let Some(session_id) = transcript_path.file_stem().and_then(|stem| stem.to_str())
            else {
                continue;
            };
            if !is_canonical_session_uuid(session_id) {
                continue;
            }
            let modified_at_unix_seconds = transcript_entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_secs()).ok());
            records.push(serde_json::json!({
                "source": "storedTranscript",
                "sessionId": session_id,
                "project": project_name,
                "updatedAtUnixSeconds": modified_at_unix_seconds,
            }));
        }
    }
    Ok(records)
}

fn is_canonical_session_uuid(session_id: &str) -> bool {
    let bytes = session_id.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes.get(index) == Some(&b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}
