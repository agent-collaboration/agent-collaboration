//! Private fixture input selects the real Inbox quota for one debug Host lifetime.
use std::{io, num::NonZeroU64, path::Path};

pub(crate) const FILE_NAME: &str = "messaging-test-inbox-limit.json";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InboxLimitInput {
    inbox_bytes: NonZeroU64,
}

pub(crate) fn load(directory: &Path) -> io::Result<Option<NonZeroU64>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if !directory.is_absolute() {
        return Err(io::Error::other(
            "debug Inbox limit directory must be absolute",
        ));
    }
    let path = directory.join(FILE_NAME);
    let file = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let parent = std::fs::symlink_metadata(directory)?;
    let owner = rustix::process::geteuid().as_raw();
    if !parent.is_dir()
        || parent.uid() != owner
        || parent.permissions().mode() & 0o077 != 0
        || !file.is_file()
        || file.uid() != owner
        || file.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "debug Inbox limit requires an owner-controlled file in a private directory",
        ));
    }
    let input: InboxLimitInput =
        serde_json::from_slice(&std::fs::read(path)?).map_err(io::Error::other)?;
    Ok(Some(input.inbox_bytes))
}

#[cfg(test)]
#[path = "debug_inbox_limit_test.rs"]
mod tests;
