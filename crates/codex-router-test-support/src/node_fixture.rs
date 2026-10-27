//! Node fixture launchers retain the invoked test executable and a restricted child PATH.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub fn node_executable() -> io::Result<PathBuf> {
    let output = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("fixture Node executable unavailable"));
    }
    let text = String::from_utf8(output.stdout).map_err(io::Error::other)?;
    Path::new(text.trim()).canonicalize()
}

#[must_use]
pub fn fixture_program(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../verification/fixtures")
        .join(name)
}

pub fn write_node_fixture(
    executable: &Path,
    program: &str,
    configuration: &serde_json::Value,
) -> io::Result<()> {
    let node = node_executable()?;
    let program = fixture_program(program).canonicalize()?;
    let import = serde_json::to_string(&program.to_string_lossy()).map_err(io::Error::other)?;
    let configuration = serde_json::to_string(configuration).map_err(io::Error::other)?;
    fs::write(
        executable,
        format!(
            "#!{}\nimport {{ runFixture }} from {import};\nawait runFixture({configuration});\n",
            node.display()
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
