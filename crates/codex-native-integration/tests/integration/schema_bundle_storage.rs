use codex_native_integration::NativeSchemaBundle;
use std::{collections::BTreeMap, os::unix::fs::DirBuilderExt};

#[test]
fn publication_is_complete_idempotent_and_never_overwrites_corruption() {
    // Arrange.
    let root = std::env::temp_dir().join(format!("schema-storage-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        b"{}".to_vec(),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    // Act.
    let path = bundle
        .publish(&root, std::time::SystemTime::now())
        .unwrap_or_else(|error| panic!("publish: {error}"));
    let contents = std::fs::read(&path);
    let repeated = bundle.publish(&root, std::time::SystemTime::now());
    std::fs::write(&path, b"corrupted").unwrap_or_else(|error| panic!("corrupt: {error}"));
    let rejected = bundle.publish(&root, std::time::SystemTime::now());
    let retained = std::fs::read(&path).unwrap_or_else(|error| panic!("read: {error}"));
    std::fs::remove_file(&path).unwrap_or_else(|error| panic!("file cleanup: {error}"));
    std::fs::remove_dir(&root).unwrap_or_else(|error| panic!("directory cleanup: {error}"));
    // Assert.
    assert_eq!(contents.ok().as_deref(), Some(bundle.canonical_bytes()));
    assert_eq!(repeated.ok(), Some(path));
    assert!(rejected.is_err());
    assert_eq!(retained, b"corrupted");
}

const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const THIRTY_DAYS: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

#[expect(
    clippy::panic,
    reason = "A fixture that cannot be prepared fails the test."
)]
fn private_root() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("private root: {error}"));
    root
}

#[expect(
    clippy::panic,
    reason = "A fixture that cannot be prepared fails the test."
)]
fn bundle(text: &str) -> NativeSchemaBundle {
    NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        text.as_bytes().to_vec(),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"))
}

#[expect(
    clippy::panic,
    reason = "A fixture that cannot be prepared fails the test."
)]
fn set_last_use(path: &std::path::Path, at: std::time::SystemTime) {
    std::fs::File::open(path)
        .and_then(|file| file.set_modified(at))
        .unwrap_or_else(|error| panic!("age {}: {error}", path.display()));
}

#[expect(
    clippy::panic,
    reason = "A fixture that cannot be prepared fails the test."
)]
fn last_use(path: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|error| panic!("mtime {}: {error}", path.display()))
}

#[test]
fn republishing_an_existing_bundle_records_its_use() {
    let root = private_root();
    let in_use = bundle(r#"{"in":"use"}"#);
    let now = std::time::SystemTime::now();
    let path = in_use
        .publish(root.path(), now - 40 * DAY)
        .unwrap_or_else(|error| panic!("first publish: {error}"));
    assert_eq!(last_use(&path), now - 40 * DAY);

    let again = in_use
        .publish(root.path(), now)
        .unwrap_or_else(|error| panic!("republish: {error}"));

    assert_eq!(again, path);
    assert_eq!(last_use(&path), now, "reusing a digest is a use");
}

#[test]
fn retirement_removes_only_bundles_unused_for_thirty_days_and_never_the_one_in_use() {
    let root = private_root();
    let now = std::time::SystemTime::now();
    let in_use = bundle(r#"{"case":"in use, last use long ago"}"#);
    let recent = bundle(r#"{"case":"used 29 days ago"}"#);
    let stale = bundle(r#"{"case":"unused for 31 days"}"#);
    let in_use_path = in_use
        .publish(root.path(), now - 40 * DAY)
        .unwrap_or_else(|error| panic!("in use: {error}"));
    let recent_path = recent
        .publish(root.path(), now - 29 * DAY)
        .unwrap_or_else(|error| panic!("recent: {error}"));
    let stale_path = stale
        .publish(root.path(), now - 31 * DAY)
        .unwrap_or_else(|error| panic!("stale: {error}"));
    let unrelated = root.path().join("service.json");
    std::fs::write(&unrelated, b"{}").unwrap_or_else(|error| panic!("unrelated: {error}"));
    set_last_use(&unrelated, now - 400 * DAY);

    let report = codex_native_integration::retire_unused_schema_bundles(
        root.path(),
        &in_use,
        now,
        THIRTY_DAYS,
    );

    assert_eq!(report.removed, vec![stale_path.clone()]);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(in_use_path.is_file(), "the bundle in use is kept");
    assert!(
        recent_path.is_file(),
        "a bundle used within 30 days is kept"
    );
    assert!(
        !stale_path.exists(),
        "a bundle unused for 31 days is removed"
    );
    assert!(unrelated.is_file(), "only schema bundles are considered");
}
