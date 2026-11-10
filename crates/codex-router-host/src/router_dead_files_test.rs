use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
fn dead_files_and_export_folders_are_found_by_name_and_nothing_else_is() -> TestResult {
    // Arrange: every dead name, an export folder, and files the Router still reads.
    let root = tempfile::tempdir()?;
    let directory = crate::router_collaboration_directory(root.path());
    std::fs::create_dir_all(directory.join("native-schema-export-0001"))?;
    let dead = [
        "approval-history.json",
        "approval-routes.json",
        "control-schema-ab12.json",
        "interaction-history.json",
        "project-board.sqlite",
        "project-board.sqlite-shm",
        "project-board.sqlite-wal",
    ];
    let live = [
        "interaction.sqlite",
        "enrollment-reset.json",
        "control-schema-ab12.json.tmp",
        "service.json",
        "native-schema-export-file",
    ];
    for name in dead.iter().chain(live.iter()) {
        std::fs::write(directory.join(name), name)?;
    }

    // Act
    let found = router_dead_files(root.path())?;
    let folders = router_schema_export_folders(root.path())?;

    // Assert
    let expected: Vec<PathBuf> = dead.iter().map(|name| directory.join(name)).collect();
    assert_eq!(found, expected);
    assert_eq!(folders, vec![directory.join("native-schema-export-0001")]);
    Ok(())
}

#[test]
fn a_root_without_a_collaboration_directory_has_no_dead_files() -> TestResult {
    let root = tempfile::tempdir()?;
    if !router_dead_files(root.path())?.is_empty() {
        return Err("a missing directory holds no dead files".into());
    }
    Ok(())
}
