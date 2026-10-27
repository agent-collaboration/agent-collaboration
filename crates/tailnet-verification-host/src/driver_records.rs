use std::fs::{OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::driver_error::DriverResult;

pub fn read_record<TRecord: DeserializeOwned>(path: &Path) -> DriverResult<TRecord> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

pub fn save_record<TRecord: Serialize>(path: &Path, value: &TRecord) -> DriverResult<()> {
    let text = serde_json::to_string_pretty(value)?;
    let mut ascii = String::new();
    for character in text.chars() {
        if character >= '\u{7f}' {
            use std::fmt::Write;
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                let _ = write!(ascii, "\\u{unit:04x}");
            }
        } else {
            ascii.push(character);
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    writeln!(file, "{ascii}")?;
    file.set_permissions(Permissions::from_mode(0o600))?;
    Ok(())
}

pub fn sha256(path: &Path) -> DriverResult<String> {
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}
