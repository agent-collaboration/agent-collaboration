//! A child declaration never substitutes for the real parent's terminal result.
use super::*;

#[test]
fn final_login_audit_requires_one_parent_observed_exact_exit() {
    for code in [0u8, 130, 7] {
        let actual = json!({"childPid":91,"waitOrdinal":1,"status":{"kind":"exited","code":code,"rawStatus":i64::from(code)<<8}});
        validation::validate_login_parent_exit(code, &actual).unwrap();
        for wrong in [
            Value::Null,
            json!({"exitCode":code}),
            json!({"childPid":91,"waitOrdinal":2,"status":{"kind":"exited","code":code,"rawStatus":i64::from(code)<<8}}),
            json!({"childPid":0,"waitOrdinal":1,"status":{"kind":"exited","code":code,"rawStatus":i64::from(code)<<8}}),
            json!({"childPid":91,"waitOrdinal":1,"status":{"kind":"signaled","signal":2,"rawStatus":2}}),
            json!({"childPid":91,"waitOrdinal":1,"status":{"kind":"wait-failed","message":"wait refused"}}),
            json!({"childPid":91,"waitOrdinal":1,"status":{"kind":"exited","code":255,"rawStatus":65280}}),
        ] {
            assert!(
                validation::validate_login_parent_exit(code, &wrong).is_err(),
                "planned/missing/wrong parent result must fail: {wrong}"
            );
        }
    }
}
