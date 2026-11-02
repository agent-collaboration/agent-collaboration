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
