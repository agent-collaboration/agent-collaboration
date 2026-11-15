//! Sole owner of a bridge: EOF then OS wait; nextest bounds the complete process group.
use std::{
    process::Child,
    sync::{Mutex, PoisonError},
};
pub(crate) struct Process {
    child: Mutex<Option<Child>>,
    output: Mutex<Option<std::thread::JoinHandle<()>>>,
    errors: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl Process {
    pub(crate) fn set_error_drain(&self, drain: Option<std::thread::JoinHandle<()>>) {
        *self.errors.lock().unwrap_or_else(PoisonError::into_inner) = drain;
    }
    pub(crate) fn set_output_drain(&self, drain: std::thread::JoinHandle<()>) {
        *self.output.lock().unwrap_or_else(PoisonError::into_inner) = Some(drain);
    }
    pub(crate) fn own(child: Child) -> Self {
        Self {
            child: Mutex::new(Some(child)),
            output: Mutex::new(None),
            errors: Mutex::new(None),
        }
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
