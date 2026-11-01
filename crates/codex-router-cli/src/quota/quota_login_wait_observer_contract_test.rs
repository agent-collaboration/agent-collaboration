//! Real private children exercise the same terminal-wait owner as the CLI login loop.
use super::*;
use crate::quota::quota_login_child::{await_quota_login_child, login_result};

async fn actual_wait(
    script: &str,
    observer: Option<&LoginWaitObserver>,
) -> io::Result<(u32, QuotaLoginResult)> {
    let child = tokio::process::Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let child_pid = child
        .id()
        .ok_or_else(|| io::Error::other("spawned private child has no PID"))?;
    let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = match observer {
        Some(observer) => {
            observer
                .scope(await_quota_login_child(child, interrupt))
                .await
        }
        None => await_quota_login_child(child, interrupt).await,
    };
    Ok((child_pid, result))
}

#[tokio::test]
async fn login_wait_observer_copies_actual_child_status_before_equal_mapping_input() {
    for (script, code, expected) in [
        ("exit 0", 0, QuotaLoginResult::Succeeded),
        ("exit 130", 130, QuotaLoginResult::Cancelled),
        (
            "exit 7",
            7,
            QuotaLoginResult::Failed("exit status: 7".to_owned()),
        ),
    ] {
        let observer = LoginWaitObserver::default();
        let (child_pid, result) = actual_wait(script, Some(&observer)).await.unwrap();
        let parent = observer.validated_parent_exit().unwrap();
        assert_eq!(parent.child_pid, Some(child_pid));
        assert_eq!(parent.wait_ordinal, 1);
        assert_eq!(
            parent.status,
            LoginWaitStatus::Exited {
                code,
                raw_status: code << 8
            }
        );
        assert_eq!(result, expected);
        let events = observer.observations().unwrap();
        assert!(
            matches!(&events[2],LoginWaitObservation::MappingReturned(mapped) if mapped==&result)
        );
        let wire = serde_json::to_value(&parent).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"childPid":child_pid,"waitOrdinal":1,"status":{"kind":"exited","code":code,"rawStatus":code<<8}})
        );
    }
}

#[tokio::test]
async fn login_wait_observer_preserves_signal_and_wait_error_and_stays_task_scoped() {
    let observer = LoginWaitObserver::default();
    let (child_pid, result) = actual_wait("kill -INT $$", Some(&observer)).await.unwrap();
    let parent = observer.validated_parent_exit().unwrap();
    assert_eq!(parent.child_pid, Some(child_pid));
    assert_eq!(
        parent.status,
        LoginWaitStatus::Signaled {
            signal: 2,
            raw_status: 2
        }
    );
    assert_eq!(result, QuotaLoginResult::Cancelled);
    let errors = LoginWaitObserver::default();
    let result = errors
        .scope(async {
            let error = Err(io::Error::other("private wait failed"));
            parent_wait_returned(Some(91), &error);
            login_result(error)
        })
        .await;
    assert_eq!(
        errors.validated_parent_exit().unwrap().status,
        LoginWaitStatus::WaitFailed {
            message: "private wait failed".to_owned()
        }
    );
    assert_eq!(
        result,
        QuotaLoginResult::Failed("private wait failed".to_owned())
    );
    let before_unscoped = observer.observations().unwrap();
    assert_eq!(
        actual_wait("exit 0", None).await.unwrap().1,
        QuotaLoginResult::Succeeded
    );
    assert_eq!(
        observer.observations().unwrap(),
        before_unscoped,
        "ordinary scope must not append to the completed observer"
    );
}

#[tokio::test]
async fn login_wait_receipt_failure_is_retained_for_the_final_fixture_audit() {
    let observer = LoginWaitObserver::with_parent_exit_sink(Arc::new(|_| {
        Err("private receipt refused".to_owned())
    }));
    assert_eq!(
        actual_wait("exit 0", Some(&observer)).await.unwrap().1,
        QuotaLoginResult::Succeeded
    );
    assert!(
        observer
            .validated_parent_exit()
            .unwrap_err()
            .contains("private receipt refused")
    );
}
