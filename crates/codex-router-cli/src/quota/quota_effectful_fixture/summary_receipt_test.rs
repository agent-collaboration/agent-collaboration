//! Large receipt bytes and completion visibility stay real; no elapsed-time assertion.
use super::super::receipts::{finish_receipt, write_summary};
use super::*;
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};

fn large_summary() -> Value {
    let cells = (0..36)
        .flat_map(|row| (0..120).map(move |column| json!([column, row])))
        .collect::<Vec<_>>();
    let poses = (1..=80).map(|frame| json!({"mount":1,"published":{"frame":frame,"surfaces":[{"surface":"Screen","cells":cells},{"surface":"Overlay(OverlayId(1))","cells":cells}]}})).collect::<Vec<_>>();
    json!({"scenario":"login-succeeded","publishedPoses":poses,"outcomes":[{"kind":"quit"}]})
}

struct CountedFile {
    file: File,
    writes: usize,
    bytes: usize,
}
impl Write for CountedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        let written = self.file.write(bytes)?;
        self.bytes += written;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[test]
fn large_summary_preserves_every_pose_without_per_token_file_writes() {
    // Arrange: two actual 120x36 surface cell sets on each of 80 publications.
    let summary = large_summary();
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("receipt.json");
    let mut writer = CountedFile {
        file: absent_file(&path).unwrap(),
        writes: 0,
        bytes: 0,
    };
    // Act: use the same serializer that the compiled effectful fixture calls.
    let started = Instant::now();
    write_summary(&mut writer, &summary).unwrap();
    let elapsed = started.elapsed();
    // Assert: real file round trip, newline and broad write batching, without a time limit.
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 5_000_000);
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), summary);
    assert_eq!(writer.bytes, bytes.len());
    eprintln!(
        "SUMMARY_WRITE_TIMING bytes={} writes={} elapsed_us={}",
        writer.bytes,
        writer.writes,
        elapsed.as_micros()
    );
    assert!(
        writer.writes < bytes.len() / 1024,
        "large receipt used {} underlying file writes for {} bytes",
        writer.writes,
        bytes.len()
    );
}

struct ObserveCompletion {
    receipt: PathBuf,
    expected: Arc<Value>,
    observed: Arc<Mutex<Vec<Value>>>,
    event: Vec<u8>,
}
impl Write for ObserveCompletion {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.event.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let record: Value = serde_json::from_slice(&self.event).map_err(io::Error::other)?;
        if record["event"]["kind"] != "fixture-finished" {
            return Err(io::Error::other("unexpected completion event"));
        }
        let bytes = std::fs::read(&self.receipt)?;
        if bytes.last() != Some(&b'\n') {
            return Err(io::Error::other(
                "receipt was incomplete when completion flushed",
            ));
        }
        let actual: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if actual != *self.expected {
            return Err(io::Error::other("receipt changed before completion"));
        }
        self.observed
            .lock()
            .map_err(|_| io::Error::other("completion observer poisoned"))?
            .push(record);
        self.event.clear();
        Ok(())
    }
}

#[test]
fn fixture_finished_observer_can_read_the_complete_large_receipt() {
    // Arrange: observe real file contents at the event publication boundary.
    let summary = Arc::new(large_summary());
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("receipt.json");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let events = FixtureEvents::with_writer(ObserveCompletion {
        receipt: path.clone(),
        expected: summary.clone(),
        observed: observed.clone(),
        event: Vec::new(),
    });
    // Act: exactly the fixture's receipt/sync/completion path.
    finish_receipt(
        absent_file(&path).unwrap(),
        &summary,
        Scenario::LoginSucceeded,
        &events,
    )
    .unwrap();
    // Assert: one completion with the adopted envelope, only after complete bytes.
    assert_eq!(
        *observed.lock().unwrap(),
        vec![
            json!({"sequence":1,"event":{"kind":"fixture-finished","scenario":"login-succeeded","outcome":"quit"}})
        ]
    );
}

#[test]
fn summary_write_failure_does_not_publish_fixture_finished() {
    // Arrange: a real descriptor that refuses writes, and an ordinary event journal.
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("receipt.json");
    std::fs::write(&path, []).unwrap();
    let journal_path = output.path().join("events.jsonl");
    let events = FixtureEvents::new(absent_file(&journal_path).unwrap());
    // Act: a small summary also exercises a pending buffered tail on flush.
    let result = finish_receipt(
        File::open(&path).unwrap(),
        &json!({"outcomes":[{"kind":"quit"}]}),
        Scenario::LoginSucceeded,
        &events,
    );
    // Assert: failure propagates and no successful completion is invented.
    assert!(result.is_err());
    assert!(std::fs::read(path).unwrap().is_empty());
    assert!(std::fs::read(journal_path).unwrap().is_empty());
}

#[test]
#[ignore = "opt-in real-file timing over a caller-supplied retained JSON workload; no native TUI"]
fn retained_native_receipt_prefix_writer_timing() {
    // Arrange: an explicit retained workload, separate from the synthetic regression oracle.
    let input = std::env::var_os("QUOTA_SUMMARY_TIMING_INPUT").expect("explicit workload path");
    let summary: Value = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("receipt.json");
    let mut writer = CountedFile {
        file: absent_file(&path).unwrap(),
        writes: 0,
        bytes: 0,
    };
    // Act: measure the same writer and real sync, without asserting elapsed time.
    let started = Instant::now();
    write_summary(&mut writer, &summary).unwrap();
    let write_elapsed = started.elapsed();
    writer.file.sync_all().unwrap();
    let total_elapsed = started.elapsed();
    // Assert: every supplied observation survives the real file round trip.
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), summary);
    eprintln!(
        "NATIVE_PREFIX_WRITE_TIMING bytes={} writes={} write_us={} write_sync_us={}",
        writer.bytes,
        writer.writes,
        write_elapsed.as_micros(),
        total_elapsed.as_micros()
    );
}
