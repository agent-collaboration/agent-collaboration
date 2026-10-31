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
mod tests {
    use super::*;
    #[derive(Default)]
    struct WriteTrace {
        bytes: Vec<u8>,
        flushes: Vec<Vec<u8>>,
        fail_flush: bool,
    }
    struct TracedWriter(Arc<Mutex<WriteTrace>>);
    impl Write for TracedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = bytes.len().min(2);
            self.0
                .lock()
                .map_err(|_| io::Error::other("write trace poisoned"))?
                .bytes
                .extend(bytes.iter().take(count).copied());
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            let mut trace = self
                .0
                .lock()
                .map_err(|_| io::Error::other("write trace poisoned"))?;
            if trace.fail_flush {
                return Err(io::Error::other("frame flush rejected"));
            }
            let bytes = trace.bytes.clone();
            trace.flushes.push(bytes);
            Ok(())
        }
    }
    #[test]
    fn short_writes_keep_the_marker_after_the_successful_underlying_frame_flush() {
        let trace = Arc::new(Mutex::new(WriteTrace::default()));
        let state = DispatchFrameState::default();
        state.record(7);
        assert!(state.prepare_frame());
        let mut writer = InputDispatchFrameWriter::new(TracedWriter(trace.clone()), state);
        writer.write_all(b"FRAME\x1b[?2026l").unwrap();
        writer.flush().unwrap();
        let trace = trace.lock().unwrap();
        assert_eq!(
            trace.flushes[0], b"FRAME\x1b[?2026l",
            "the frame was flushed before any marker bytes"
        );
        assert_eq!(
            trace.flushes[1],
            b"FRAME\x1b[?2026l\x1b]7770;dispatch=7\x1b\\"
        );
    }
    #[test]
    fn pending_receipts_and_unrelated_flushes_never_publish_a_premature_marker() {
        let trace = Arc::new(Mutex::new(WriteTrace::default()));
        let state = DispatchFrameState::default();
        state.record(1);
        let mut writer = InputDispatchFrameWriter::new(TracedWriter(trace.clone()), state.clone());
        writer.write_all(b"\x1b[?2026l").unwrap();
        writer.flush().unwrap();
        assert!(
            !trace
                .lock()
                .unwrap()
                .bytes
                .windows(4)
                .any(|bytes| bytes == b"7770")
        );
        assert!(state.prepare_frame());
        writer.write_all(b"other output").unwrap();
        writer.flush().unwrap();
        assert!(
            !trace
                .lock()
                .unwrap()
                .bytes
                .windows(4)
                .any(|bytes| bytes == b"7770")
        );
        writer.write_all(b"\x1b[?2026l").unwrap();
        writer.flush().unwrap();
        let expected = trace.lock().unwrap().bytes.clone();
        assert!(expected.ends_with(b"\x1b]7770;dispatch=1\x1b\\"));
        writer.flush().unwrap();
        assert_eq!(
            trace.lock().unwrap().bytes,
            expected,
            "a later flush never duplicates the marker"
        );
    }
    #[test]
    fn a_failed_frame_flush_returns_the_error_without_writing_a_marker() {
        let trace = Arc::new(Mutex::new(WriteTrace {
            fail_flush: true,
            ..Default::default()
        }));
        let state = DispatchFrameState::default();
        state.record(3);
        assert!(state.prepare_frame());
        let mut writer = InputDispatchFrameWriter::new(TracedWriter(trace.clone()), state);
        writer.write_all(b"FRAME\x1b[?2026l").unwrap();
        assert_eq!(
            writer.flush().unwrap_err().to_string(),
            "frame flush rejected"
        );
        assert_eq!(trace.lock().unwrap().bytes, b"FRAME\x1b[?2026l");
        assert!(trace.lock().unwrap().flushes.is_empty());
    }
    #[test]
    fn an_invalid_underlying_write_count_returns_an_error_instead_of_indexing() {
        struct InvalidCountWriter;
        impl Write for InvalidCountWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len() + 1)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer =
            InputDispatchFrameWriter::new(InvalidCountWriter, DispatchFrameState::default());
        assert_eq!(
            writer.write(b"x").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
