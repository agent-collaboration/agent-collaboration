use super::*;
use std::time::Duration;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn scratch_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch root")
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .expect("modification time")
}

fn age(path: &Path, at: SystemTime) {
    File::open(path)
        .and_then(|file| file.set_modified(at))
        .expect("age fixture");
}

#[test]
fn preparing_a_scope_creates_a_private_folder_and_records_its_use() {
    let root = scratch_root();
    let folder = root.path().join("session-a");

    prepare_scratch_scope(&folder).expect("prepare");

    let mode = std::fs::metadata(&folder)
        .expect("folder")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    assert!(folder.join(SCRATCH_LAST_USED_FILE_NAME).is_file());
}

#[test]
fn preparing_an_existing_scope_records_a_new_use_and_keeps_its_contents() {
    let root = scratch_root();
    let folder = root.path().join("thread-root");
    prepare_scratch_scope(&folder).expect("first use");
    std::fs::write(folder.join("notes.md"), b"kept").expect("content");
    let marker = folder.join(SCRATCH_LAST_USED_FILE_NAME);
    let old = SystemTime::now() - 40 * DAY;
    age(&marker, old);

    prepare_scratch_scope(&folder).expect("second use");

    assert!(
        modified(&marker) > old + 39 * DAY,
        "the use stamp moved to now"
    );
    assert_eq!(
        std::fs::read(folder.join("notes.md")).expect("kept"),
        b"kept"
    );
}

#[test]
fn an_exclusive_hold_excludes_another_exclusive_and_a_shared_hold_excludes_the_sweep() {
    let root = scratch_root();
    let held = try_lock_scratch_scope_exclusively(root.path(), "scope")
        .expect("lock")
        .expect("first exclusive hold");

    let second = try_lock_scratch_scope_exclusively(root.path(), "scope").expect("lock");
    assert!(
        second.is_none(),
        "a second exclusive hold must not be granted"
    );
    drop(held);

    let creator = open_scope_lock(root.path(), "scope").expect("creator lock");
    creator.lock_shared().expect("creator shared hold");
    let sweep = try_lock_scratch_scope_exclusively(root.path(), "scope").expect("lock");
    assert!(
        sweep.is_none(),
        "the sweep must skip a scope a creator holds"
    );
    drop(creator);
    assert!(
        try_lock_scratch_scope_exclusively(root.path(), "scope")
            .expect("lock")
            .is_some()
    );
}

#[test]
fn the_last_change_is_the_newest_modification_anywhere_in_the_folder() {
    let root = scratch_root();
    let folder = root.path().join("scope");
    std::fs::create_dir_all(folder.join("nested/deeper")).expect("tree");
    let file = folder.join("nested/deeper/recent.txt");
    std::fs::write(&file, b"x").expect("file");
    let old = SystemTime::now() - 40 * DAY;
    let recent = SystemTime::now() - 2 * DAY;
    for path in [
        folder.clone(),
        folder.join("nested"),
        folder.join("nested/deeper"),
    ] {
        age(&path, old);
    }
    age(&file, recent);

    assert_eq!(
        scratch_scope_last_change(&folder).expect("last change"),
        recent
    );
}

#[test]
fn an_unreadable_entry_makes_the_last_change_unknown() {
    let root = scratch_root();
    let folder = root.path().join("scope");
    let locked = folder.join("locked");
    std::fs::create_dir_all(locked.join("inside")).expect("tree");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("unreadable");

    let result = scratch_scope_last_change(&folder);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))
        .expect("restore for cleanup");
    assert!(result.is_err(), "unknown use must not read as old");
}
