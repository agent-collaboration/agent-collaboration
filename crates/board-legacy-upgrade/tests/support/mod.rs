#![allow(dead_code)]
use board_legacy_upgrade::{PreparedUpgrade, prepare_sources};
use fleet_admission::ImportProvenance;
use message_board::*;
use std::path::{Path, PathBuf};
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub fn create_host_identity(root: &Path) -> TestResult<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let directory = codex_router_host::router_collaboration_directory(root);
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    collaboration_service::load_service_identity(&directory)?;
    Ok(directory.join("service-identity.json"))
}
pub fn fixture_copy(name: &str) -> TestResult<(tempfile::TempDir, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join(name);
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &destination,
    )?;
    Ok((directory, destination))
}
pub async fn prepare(name: &str) -> TestResult<PreparedUpgrade> {
    let (_directory, path) = fixture_copy(name)?;
    prepare_path(&path).await
}
pub async fn prepare_path(path: &Path) -> TestResult<PreparedUpgrade> {
    Ok(prepare_sources(
        &[path.to_owned()],
        provenance("legacy fixture machine")?,
        HumanId::try_from("legacy-owner".to_owned())?,
    )
    .await?)
}
pub fn provenance(label: &str) -> TestResult<ImportProvenance> {
    Ok(ImportProvenance {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
        machine: MachineLabel::try_from(label.to_owned())?,
    })
}
