//! Sole owner of a bridge: EOF then OS wait; nextest bounds the complete process group.
use std::{
    io::Write,
    process::Child,
    sync::{Mutex, PoisonError, mpsc},
};
pub(crate) struct Process {
    child: Mutex<Option<Child>>,
    output: Mutex<Option<std::thread::JoinHandle<()>>>,
    errors: Mutex<Option<std::thread::JoinHandle<()>>>,
    replies: mpsc::Sender<Option<String>>,
    responses: Mutex<mpsc::Receiver<Option<String>>>,
    command_turn: Mutex<()>,
}
impl Process {
    pub(crate) fn set_error_drain(&self, drain: Option<std::thread::JoinHandle<()>>) {
        *self.errors.lock().unwrap_or_else(PoisonError::into_inner) = drain;
    }
    pub(crate) fn set_output_drain(&self, drain: std::thread::JoinHandle<()>) {
        *self.output.lock().unwrap_or_else(PoisonError::into_inner) = Some(drain);
    }
    pub(crate) fn own(child: Child) -> Self {
        let (replies, responses) = mpsc::channel();
        Self {
            child: Mutex::new(Some(child)),
            output: Mutex::new(None),
            errors: Mutex::new(None),
            replies,
            responses: Mutex::new(responses),
            command_turn: Mutex::new(()),
        }
    }
    pub(crate) fn reply_sender(&self) -> mpsc::Sender<Option<String>> {
        self.replies.clone()
    }
    pub(crate) fn exchange(&self, request: &[u8]) -> Result<String, central_storage::StoreFailure> {
        let _turn = self
            .command_turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.with_child(|child| {
            let input = child
                .stdin
                .as_mut()
                .ok_or(central_storage::StoreFailure::Unavailable)?;
            input
                .write_all(request)
                .and_then(|()| input.write_all(b"\n"))
                .and_then(|()| input.flush())
                .map_err(|_| central_storage::StoreFailure::Unavailable)
        })?;
        self.responses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv()
            .ok()
            .flatten()
            .ok_or(central_storage::StoreFailure::Unavailable)
    }
    pub(crate) fn with_child<T>(&self, action: impl FnOnce(&mut Child) -> T) -> T {
        action(
            self.child
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .unwrap(),
        )
    }

    pub(crate) fn stop(&self) -> bool {
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            // EOF is the bridge's disposal command, including parent unwinding.
            drop(child.stdin.take());
            let stopped = child.wait().is_ok_and(|status| status.success());
            if let Some(output) = self
                .output
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                && output.join().is_err()
            {
                return false;
            }
            if let Some(errors) = self
                .errors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                assert!(errors.join().is_ok());
            }
            return stopped;
        }
        true
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        assert!(
            self.stop() || std::thread::panicking(),
            "owned bridge did not close successfully"
        );
    }
}

#[cfg(test)]
#[path = "workerd_bridge_process_test.rs"]
mod tests;
