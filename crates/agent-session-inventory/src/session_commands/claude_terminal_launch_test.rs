use super::*;

#[test]
fn background_receipt_accepts_recorded_named_and_unnamed_vendor_output() {
    for receipt in [
        "backgrounded · a1b2c3d4 · <redacted> (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
        "backgrounded · a1b2c3d4 (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
    ] {
        let handle = parse_background_receipt(receipt.as_bytes())
            .expect("recorded Claude2.1.312 background receipt");
        assert_eq!(String::from(handle), "a1b2c3d4");
    }
}

#[test]
fn background_receipt_does_not_guess_or_accept_ambiguous_handles() {
    for receipt in [
        "",
        "  claude attach a1b2c3d4 open in this terminal\n",
        "backgrounded · not-a-job · name\n",
        "backgrounded · a1b2c3d4 · one\nbackgrounded · deadbeef · two\n",
    ] {
        let message = parse_background_receipt(receipt.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(message.contains("claude agents") && message.contains("do not start another copy"));
    }
}
