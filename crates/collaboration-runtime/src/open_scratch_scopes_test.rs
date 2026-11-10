use super::*;
use std::time::{Duration, SystemTime};

fn last_used(folder: &Path) -> SystemTime {
    std::fs::metadata(folder.join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME))
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|error| panic!("use stamp: {error}"))
}

fn age_use_stamp(folder: &Path, at: SystemTime) {
    std::fs::File::open(folder.join(codex_native_integration::SCRATCH_LAST_USED_FILE_NAME))
        .and_then(|file| file.set_modified(at))
        .unwrap_or_else(|error| panic!("age use stamp: {error}"));
}

#[test]
fn a_scope_stays_open_until_every_binding_on_it_closes() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
    let folder = root.path().join("scratch").join("thread-root");
    let register = OpenScratchScopes::new();

    register.opened(&folder);
    register.opened(&folder);
    register.closed(&folder);
    let open_after_one_close = register.is_open(&folder);
    register.closed(&folder);

    assert!(
        open_after_one_close,
        "a second connection keeps the scope open"
    );
    assert!(!register.is_open(&folder));
}

#[test]
fn opening_and_closing_record_a_use_in_the_folder() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("root: {error}"));
    let folder = root.path().join("scratch").join("session-a");
    let register = OpenScratchScopes::new();
    let long_ago = SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60);

    register.opened(&folder);
    let opened_stamp = last_used(&folder);
    age_use_stamp(&folder, long_ago);
    register.closed(&folder);

    assert!(
        opened_stamp > long_ago,
        "opening creates the folder and stamps it"
    );
    assert!(last_used(&folder) > long_ago, "closing stamps a new use");
}
