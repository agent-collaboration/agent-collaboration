use super::*;
use chrono::{Duration, Utc};

fn declared_plan() -> DriverResult<D1NativeReadinessPlan> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    Ok(D1NativeReadinessPlan {
        run_root: repository.join("tmp/d1-verify/allocation/plan-validation-only"),
        repository,
        native: D1NativeArtifact {
            path: PathBuf::from("/bin/echo"),
            sha256: "0".repeat(64),
            version: "fixture-only".into(),
        },
        auth_policy: D1AuthPolicy::AwaitingOwner {},
        deadline: (Utc::now() + Duration::hours(1)).fixed_offset(),
    })
}

#[test]
fn d1_plan_preserves_allocation_and_refuses_normal_or_traversing_roots() {
    let plan = declared_plan().expect("fixture plan");
    plan.validate().expect("valid allocation");
    for forbidden in [
        plan.repository.join("tmp/a"),
        plan.repository.join("tmp/d1-verify/allocation/../escape"),
        PathBuf::from("/private/tmp/not-in-the-d1-worktree"),
    ] {
        let mut changed = plan.clone();
        changed.run_root = forbidden;
        assert!(changed.validate().is_err());
    }
}

#[test]
fn d1_auth_policy_has_no_secret_field_and_shared_origin_is_loopback_only() {
    let mut plan = declared_plan().expect("fixture plan");
    plan.auth_policy = D1AuthPolicy::SharedDebugProxy {
        origin: "http://127.0.0.1:28787".into(),
    };
    plan.validate().expect("loopback proxy");
    for origin in [
        "https://example.com",
        "http://user:secret@127.0.0.1:28787",
        "http://127.0.0.1:28787/v1",
    ] {
        plan.auth_policy = D1AuthPolicy::SharedDebugProxy {
            origin: origin.into(),
        };
        assert!(plan.validate().is_err());
    }
    assert!(
        serde_json::from_value::<D1AuthPolicy>(serde_json::json!({
            "kind":"dedicatedDebugAccounts","token":"forbidden"
        }))
        .is_err()
    );
}

#[test]
fn d1_plan_rejects_expired_deadline_and_malformed_artifact() {
    let mut plan = declared_plan().expect("fixture plan");
    plan.deadline = (Utc::now() - Duration::seconds(1)).fixed_offset();
    assert!(plan.validate().is_err());
    let mut plan = declared_plan().expect("fixture plan");
    plan.native.sha256 = "z".repeat(64);
    assert!(plan.validate().is_err());
}
