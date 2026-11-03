//! Reject a successful libtest process whose exact selector matched no child case.
pub(super) fn assert_one_test_executed(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running 1 test\n")
            && (stdout.contains("1 passed;") || stdout.contains("1 failed;")),
        "the picker child selector must execute exactly one case: {stdout}"
    );
}
