//! Test-only barrier: holds a scratch creator just after it has taken the
//! scope's shared lock, so a test can observe what a sweep sees meanwhile.
//! Compiled only with the `test-support` feature, which only test builds enable.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

struct HeldCreator {
    reached: SyncSender<()>,
    release: Receiver<()>,
}

static HELD_CREATORS: Mutex<BTreeMap<PathBuf, HeldCreator>> = Mutex::new(BTreeMap::new());

/// The test's side of one held creator.
pub struct ScratchCreatorBarrier {
    reached: Receiver<()>,
    release: SyncSender<()>,
}

/// The next creator of `scope_folder` stops right after taking the shared lock
/// until the returned barrier is released.
#[must_use]
pub fn hold_next_scratch_creator(scope_folder: &Path) -> ScratchCreatorBarrier {
    let (reached_sender, reached) = sync_channel(1);
    let (release, release_receiver) = sync_channel(1);
    HELD_CREATORS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            scope_folder.to_owned(),
            HeldCreator {
                reached: reached_sender,
                release: release_receiver,
            },
        );
    ScratchCreatorBarrier { reached, release }
}

impl ScratchCreatorBarrier {
    /// Blocks until the creator holds the shared lock and has stopped.
    pub fn wait_until_held(&self) -> Result<(), std::sync::mpsc::RecvError> {
        self.reached.recv()
    }

    /// Lets the held creator continue.
    pub fn release(self) {
        let _ = self.release.send(());
    }
}

/// Called by `prepare_scratch_scope` while it holds the shared lock.
pub(crate) fn creator_holds_scope_lock(scope_folder: &Path) {
    let held = HELD_CREATORS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(scope_folder);
    if let Some(held) = held {
        let _ = held.reached.send(());
        let _ = held.release.recv();
    }
}
