//! Dispatch receipts become markers only after their next native frame flush.
use iocraft::{ComponentUpdater, Hook, Hooks};
use std::{
    io::{self, Write},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

#[derive(Default)]
struct FrameSequences {
    pending: Vec<u64>,
    prepared: Vec<u64>,
    failure: Option<io::Error>,
}
#[derive(Clone, Default)]
pub(crate) struct DispatchFrameState(Arc<Mutex<FrameSequences>>);
impl DispatchFrameState {
    pub(crate) fn record(&self, sequence: u64) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .push(sequence);
    }
    fn has_pending(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .is_empty()
    }
    fn prepare_frame(&self) -> bool {
        let mut sequences = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pending = std::mem::take(&mut sequences.pending);
        let changed = !pending.is_empty();
        sequences.prepared.extend(pending);
        changed
    }
    fn take_prepared(&self) -> Vec<u64> {
        std::mem::take(
            &mut self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .prepared,
        )
    }
    fn record_failure(&self, error: &io::Error) {
        let mut sequences = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sequences.failure.is_none() {
            sequences.failure = Some(io::Error::new(error.kind(), error.to_string()));
        }
    }
    pub(crate) fn check_failure(&self) -> io::Result<()> {
        let sequences = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &sequences.failure {
            Some(error) => Err(io::Error::new(error.kind(), error.to_string())),
            None => Ok(()),
        }
    }
}

struct DispatchFrameHook(Option<DispatchFrameState>);
impl Hook for DispatchFrameHook {
    fn poll_change(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<()> {
        if self.0.as_ref().is_some_and(DispatchFrameState::has_pending) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    fn post_component_update(&mut self, updater: &mut ComponentUpdater) {
        if self
            .0
            .as_ref()
            .is_some_and(DispatchFrameState::prepare_frame)
        {
            // The pin suppresses write_canvas when the cells are unchanged.
            // Its existing clear flag requests a complete write for a no-op.
            updater.clear_terminal_output();
        }
    }
}
pub(crate) fn use_dispatch_frames(hooks: &mut Hooks<'_, '_>, state: Option<DispatchFrameState>) {
    // Installed after InputOwner's event hook, so its poll observes receipts
    // recorded by that hook in the same pass, without another subscription.
    let hook = hooks.use_hook(|| DispatchFrameHook(state.clone()));
    hook.0 = state;
}

/// Verification-only output writer; construct it through InputDispatchReceiptSink.
pub struct InputDispatchFrameWriter<W> {
    writer: W,
    state: DispatchFrameState,
    end_prefix: usize,
    end_frame_written: bool,
}
impl<W> InputDispatchFrameWriter<W> {
    pub(crate) fn new(writer: W, state: DispatchFrameState) -> Self {
        Self {
            writer,
            state,
            end_prefix: 0,
            end_frame_written: false,
        }
    }
    fn observe(&mut self, bytes: &[u8]) {
        // The pinned crossterm backend ends and flushes every synchronized
        // frame with this exact sequence. Track only accepted bytes, including
        // fragmented/short writes; this is not a permissive terminal parser.
        const END_FRAME: &[u8] = b"\x1b[?2026l";
        for byte in bytes {
            if END_FRAME.get(self.end_prefix) == Some(byte) {
                self.end_prefix += 1;
                if self.end_prefix == END_FRAME.len() {
                    self.end_prefix = 0;
                    self.end_frame_written = true;
                }
            } else {
                self.end_prefix = usize::from(*byte == b'\x1b');
            }
        }
    }
}
impl<W: Write> InputDispatchFrameWriter<W> {
    fn write_bytes(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.writer.write(bytes)?;
        let accepted = bytes.get(..written).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "output writer returned an invalid byte count",
            )
        })?;
        self.observe(accepted);
        Ok(written)
    }
    fn flush_frame(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        if self.end_frame_written {
            self.end_frame_written = false;
            for sequence in self.state.take_prepared() {
                write!(self.writer, "\x1b]7770;dispatch={sequence}\x1b\\")?;
            }
            self.writer.flush()?;
        }
        Ok(())
    }
}
impl<W: Write> Write for InputDispatchFrameWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.write_bytes(bytes);
        if let Err(error) = &result {
            self.state.record_failure(error);
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        let result = self.flush_frame();
        if let Err(error) = &result {
            self.state.record_failure(error);
        }
        result
    }
}

#[cfg(test)]
#[path = "dispatch_frame_marker_test.rs"]
mod tests;
