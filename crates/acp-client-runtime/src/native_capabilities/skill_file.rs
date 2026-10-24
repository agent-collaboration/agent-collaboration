//! Bounded UTF-8 reads of the actual native skill file.
use std::{fs::File, io::Read, path::Path};

#[derive(Clone, Copy, Debug)]
pub(super) enum NativeSkillFileError {
    Unreadable,
    InvalidMetadata,
}

pub(super) fn read_native_skill_file(
    path: &Path,
    maximum_bytes: u64,
) -> Result<String, NativeSkillFileError> {
    let file = File::open(path).map_err(|_| NativeSkillFileError::Unreadable)?;
    let metadata = file
        .metadata()
        .map_err(|_| NativeSkillFileError::Unreadable)?;
    if !metadata.is_file() || metadata.len() > maximum_bytes {
        return Err(NativeSkillFileError::InvalidMetadata);
    }
    let mut contents = Vec::new();
    let bound = maximum_bytes
        .checked_add(1)
        .ok_or(NativeSkillFileError::InvalidMetadata)?;
    file.take(bound)
        .read_to_end(&mut contents)
        .map_err(|_| NativeSkillFileError::Unreadable)?;
    if u64::try_from(contents.len()).map_err(|_| NativeSkillFileError::InvalidMetadata)?
        > maximum_bytes
    {
        return Err(NativeSkillFileError::InvalidMetadata);
    }
    String::from_utf8(contents).map_err(|_| NativeSkillFileError::InvalidMetadata)
}
