use super::*;
pub(crate) fn verified_node_executable() -> TestResult<(PathBuf, String)> {
    let path = required_executable("QUOTA_STORYBOARD_NODE")?;
    ensure(
        std::env::var_os("QUOTA_STORYBOARD_NODE").ok_or("Node nomination missing")? == path,
        "declared Node executable must retain its canonical absolute path",
    )?;
    let expected = std::env::var("QUOTA_STORYBOARD_NODE_SHA256")?;
    ensure(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && digest_file(&path)? == expected,
        "declared Node executable SHA256 changed",
    )?;
    Ok((path, expected))
}

pub(crate) fn permanent_adapter_source(name: &str) -> TestResult<PathBuf> {
    let compiled: &[u8] = match name {
        "fixture-child.ts" => {
            include_bytes!("../../../../scripts/verification/tui/fixtures/fixture-child.ts")
        }
        _ => return Err("unknown permanent TypeScript adapter".into()),
    };
    let path = fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/verification/tui")
            .join(name),
    )?;
    ensure(
        digest_file(&path)? == format!("{:x}", Sha256::digest(compiled)),
        "permanent adapter differs from compiled source binding",
    )?;
    Ok(path)
}

pub(crate) fn required_executable(name: &str) -> TestResult<PathBuf> {
    let path = fs::canonicalize(std::env::var_os(name).ok_or_else(|| {
        std::io::Error::other(format!("{name} must nominate the exact isolated tool"))
    })?)?;
    ensure(path.is_file(), "capture tool must be an existing file")?;
    Ok(path)
}
