use super::*;
use session_event_model::{ProviderSkill, SkillInvocationName};

fn bound(id: &str, cwd: &str) -> Result<BoundSkillView, Box<dyn std::error::Error>> {
    let session = serde_json::from_value(
        json!({"endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},"sessionId":id}),
    )?;
    let snapshot = SkillView::Available {
        skills: vec![ProviderSkill {
            invocation_name: SkillInvocationName::new(id)?,
            description: id.into(),
            scope: ProviderSkillScope::System,
        }],
        errors: vec![],
    };
    let (_, receiver) = watch::channel(snapshot.clone());
    Ok(BoundSkillView {
        cwd: cwd.into(),
        session,
        attachment: SkillViewAttachment { snapshot, receiver },
    })
}

#[test]
fn default_cwd_requires_one_unambiguous_binding_and_explicit_cwds_stay_isolated() {
    let mut bindings = NativeSkillBindings::default();
    assert_eq!(bindings.list(&json!({})), Ok(json!({"data":[]})));
    let first = bindings.begin();
    bindings.complete(
        first,
        Some(bound("first", "/first").expect("first binding")),
    );
    let single = bindings.list(&json!({"cwds":[]})).expect("sole view");
    assert_eq!(single["data"][0]["cwd"], "/first");
    assert_eq!(single["data"][0]["skills"][0]["name"], "first");
    let second = bindings.begin();
    bindings.complete(
        second,
        Some(bound("second", "/second").expect("second binding")),
    );
    assert_eq!(bindings.list(&json!({})), Ok(json!({"data":[]})));
    let explicit = bindings
        .list(&json!({"cwds":["/first","/unknown","/second"]}))
        .expect("explicit views");
    assert_eq!(explicit["data"][0]["skills"][0]["name"], "first");
    assert_eq!(explicit["data"][1]["skills"], json!([]));
    assert_eq!(explicit["data"][1]["errors"], json!([]));
    assert_eq!(explicit["data"][2]["skills"][0]["name"], "second");
}

#[test]
fn overlapping_successes_are_ambiguous_in_both_response_orders_until_a_later_explicit_binding() {
    for reverse in [false, true] {
        let mut bindings = NativeSkillBindings::default();
        let first = bindings.begin();
        let second = bindings.begin();
        for token in if reverse {
            [second, first]
        } else {
            [first, second]
        } {
            bindings.complete(
                token,
                Some(
                    bound(if token == first { "first" } else { "second" }, "/same")
                        .expect("binding"),
                ),
            );
        }
        let ambiguous = bindings
            .list(&json!({"cwds":["/same"]}))
            .expect("ambiguous view");
        assert_eq!(ambiguous["data"][0]["skills"], json!([]));
        assert_eq!(
            ambiguous["data"][0]["errors"]
                .as_array()
                .expect("errors")
                .len(),
            1
        );
        assert_eq!(bindings.list(&json!({})), Ok(json!({"data":[]})));
        let resumed = bindings.begin();
        bindings.complete(
            resumed,
            Some(bound("first", "/same").expect("non-overlapping resume")),
        );
        assert_eq!(
            bindings.list(&json!({"cwds":["/same"]})).expect("rebound")["data"][0]["skills"][0]["name"],
            "first"
        );
    }
}
