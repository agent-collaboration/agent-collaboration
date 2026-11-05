use super::*;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Atomic sealing must preserve even an empty colliding directory."
)]
fn atomic_sealing_never_replaces_an_existing_empty_directory()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange two real directories on one filesystem, including an empty destination.
    let area = tempfile::tempdir()?;
    let partial = area.path().join("partial");
    let destination = area.path().join("sealed");
    std::fs::create_dir(&partial)?;
    std::fs::create_dir(&destination)?;
    std::fs::write(partial.join("original"), b"prepared content")?;
    let before = destination.metadata()?.ino();
    // Act through the same atomic operation that publishes a prepared directory.
    let result = commit_directory(area.path(), &partial, "sealed");
    // Assert the destination identity is untouched and the partial remains recoverable.
    assert!(result.is_err());
    assert_eq!(destination.metadata()?.ino(), before);
    assert!(partial.join("original").is_file());
    assert_eq!(std::fs::read_dir(destination)?.count(), 0);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An empty nested directory is still part of the archived source."
)]
fn adding_an_empty_source_directory_invalidates_its_fingerprint()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange the actual filesystem without requiring any SQLite content.
    let source = tempfile::tempdir()?;
    let fingerprint = SourceDirectoryFingerprint::capture(source.path())?;
    // Act by adding a directory that contains no file to hash.
    std::fs::create_dir(source.path().join("old-epoch-notes"))?;
    // Assert the whole-directory identity changed.
    assert!(!fingerprint.matches(source.path())?);
    Ok(())
}
