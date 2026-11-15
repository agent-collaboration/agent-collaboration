use super::*;
use std::process::{Command, Stdio};

#[test]
fn unsuccessful_bridge_exit_fails_drop_but_never_double_panics() {
    let failing = || {
        Process::own(
            Command::new("node")
                .args(["-e", "process.exit(17)"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    };
    let normal_drop = std::panic::catch_unwind(|| drop(failing()));
    assert!(
        normal_drop.is_err(),
        "unsuccessful bridge exit cannot silently pass Drop"
    );
    let already_unwinding = std::panic::catch_unwind(|| {
        let _owned = failing();
        panic!("original test panic");
    });
    let payload = already_unwinding.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"original test panic"));
}
