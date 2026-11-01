//! Executable on native and wasm32-unknown-unknown to keep path validation platform-independent.
use collaboration_protocol::ProviderWorkingDirectory;
use std::{error::Error, io};

fn main() -> Result<(), Box<dyn Error>> {
    for directory in [
        "/".to_owned(),
        "/workspace".to_owned(),
        "//workspace/../project".to_owned(),
        "/workspace/项目".to_owned(),
        format!("/{}", "a".repeat(4095)),
        format!("/{}aaa", "😀".repeat(1023)),
    ] {
        let validated =
            ProviderWorkingDirectory::try_from(directory.clone()).map_err(io::Error::other)?;
        if String::from(validated) != directory {
            return Err(io::Error::other("working directory bytes changed").into());
        }
        let serialized = serde_json::to_string(&directory)?;
        let decoded: ProviderWorkingDirectory = serde_json::from_str(&serialized)?;
        if String::from(decoded) != directory {
            return Err(io::Error::other("working directory wire bytes changed").into());
        }
    }
    for directory in [
        String::new(),
        "workspace".to_owned(),
        "./workspace".to_owned(),
        "C:\\workspace".to_owned(),
        "\\workspace".to_owned(),
        "/workspace\0project".to_owned(),
        format!("/{}", "a".repeat(4096)),
        format!("/{}aaaa", "😀".repeat(1023)),
    ] {
        if ProviderWorkingDirectory::try_from(directory.clone()).is_ok() {
            return Err(io::Error::other("invalid working directory was accepted").into());
        }
        let serialized = serde_json::to_string(&directory)?;
        if serde_json::from_str::<ProviderWorkingDirectory>(&serialized).is_ok() {
            return Err(io::Error::other("invalid working directory wire was accepted").into());
        }
    }
    Ok(())
}
