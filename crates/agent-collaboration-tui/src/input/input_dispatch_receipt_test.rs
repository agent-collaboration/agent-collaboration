use super::*;
fn facts() -> InputRouteFacts {
    InputRouteFacts {
        surface: SurfaceId::Screen,
        target: None,
        action: None,
    }
}
fn event() -> InputEventReadback {
    InputEventReadback::Resize {
        columns: 80,
        rows: 24,
    }
}

#[test]
fn receipt_path_is_explicit_new_and_has_no_creation_fallback() {
    assert!(InputDispatchReceiptSink::create("relative.jsonl").is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.jsonl");
    std::fs::write(&path, "retained evidence").unwrap();
    assert!(InputDispatchReceiptSink::create(&path).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "retained evidence");
    assert!(
        InputDispatchReceiptSink::create(directory.path().join("missing/input.jsonl")).is_err()
    );
    assert!(!directory.path().join("missing").exists());
}
#[test]
fn receipt_sequence_stops_at_the_json_safe_boundary_and_retains_the_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.jsonl");
    let sink = InputDispatchReceiptSink::create(&path).unwrap();
    sink.0.lock().unwrap().sequence = 9_007_199_254_740_990;
    sink.record(event(), facts());
    sink.flush_and_check().unwrap();
    let line = std::fs::read_to_string(&path).unwrap();
    let receipt: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(receipt["sequence"], 9_007_199_254_740_991u64);
    sink.record(event(), facts());
    assert!(
        sink.flush_and_check()
            .unwrap_err()
            .to_string()
            .contains("JSON-safe")
    );
    assert!(sink.flush_and_check().is_err());
    sink.record(event(), facts());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        line,
        "no wrapped or duplicate sequence is written"
    );
}
#[test]
fn receipt_write_errors_are_retained_for_the_fixture_after_routing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.jsonl");
    std::fs::write(&path, "").unwrap();
    let sink = InputDispatchReceiptSink(Arc::new(Mutex::new(ReceiptWriter {
        output: BufWriter::new(File::open(&path).unwrap()),
        sequence: 0,
        failure: None,
        frame_state: DispatchFrameState::default(),
    })));
    sink.record(event(), facts());
    let first = sink.flush_and_check().unwrap_err().to_string();
    assert_eq!(sink.flush_and_check().unwrap_err().to_string(), first);
    assert!(std::fs::read(path).unwrap().is_empty());
}
#[test]
fn separate_input_owners_start_their_own_sequence_at_one() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["first.jsonl", "second.jsonl"] {
        let path = directory.path().join(name);
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        sink.record(event(), facts());
        sink.flush_and_check().unwrap();
        let receipt: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(path).unwrap().trim_end()).unwrap();
        assert_eq!(receipt["sequence"], 1);
    }
}
