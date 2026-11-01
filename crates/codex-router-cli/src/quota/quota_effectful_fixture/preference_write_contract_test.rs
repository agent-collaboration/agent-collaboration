//! Repair-profile evidence never substitutes a duplicate, late or missing save for one accepted choice.
use super::*;
#[test]
fn preference_profile_audit_requires_one_actual_save_and_both_real_mount_choices() {
    let action = json!({"id":"quota:group-provider","checked":true});
    let good = json!({"nativeDispatch":[{"action":"quota:group-cycle"}],"preferenceWrites":[{"ordinal":1,"facet":"provider","result":{"kind":"saved"}}],"storedGrouping":"provider","publishedPoses":[{"mount":1,"actions":[action]},{"mount":2,"actions":[action]}]});
    validation::validate_preference_writes(Scenario::LoginSucceeded, &good).unwrap();
    let mut double = good.clone();
    double["preferenceWrites"]
        .as_array_mut()
        .unwrap()
        .push(json!({"ordinal":2,"facet":"provider","result":{"kind":"saved"}}));
    assert!(validation::validate_preference_writes(Scenario::LoginSucceeded, &double).is_err());
    let mut late = good.clone();
    late["preferenceWrites"][0]["facet"] = json!("status");
    late["storedGrouping"] = json!("status");
    assert!(validation::validate_preference_writes(Scenario::LoginSucceeded, &late).is_err());
    let mut dropped = good.clone();
    dropped["preferenceWrites"] = json!([]);
    assert!(validation::validate_preference_writes(Scenario::LoginSucceeded, &dropped).is_err());
    let mut stale = good.clone();
    stale["publishedPoses"][1]["mount"] = json!(1);
    assert!(validation::validate_preference_writes(Scenario::LoginSucceeded, &stale).is_err());
    let mut failed = good;
    failed["preferenceWrites"][0]["result"]["kind"] = json!("failed");
    assert!(validation::validate_preference_writes(Scenario::LoginSucceeded, &failed).is_err());
    validation::validate_preference_writes(
        Scenario::LoginSucceeded,
        &json!({"nativeDispatch":[],"preferenceWrites":[],"storedGrouping":null}),
    )
    .unwrap();
}
