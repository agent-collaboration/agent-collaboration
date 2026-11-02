use crate::test_support::core_models::action;
use agent_collaboration_tui::actions::{
    ActionError, ActionId, ActionRegistry, Availability, AvailabilityFn, ScopeId,
};
use iocraft::Handler;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[test]
fn duplicate_key_in_one_scope_is_rejected_including_equivalent_spellings() {
    for (first, second) in [
        ("Ctrl+N", "control+n"),
        ("Shift+Tab", "backtab"),
        ("F10", "f10"),
    ] {
        let result = ActionRegistry::new(
            ScopeId::new("screen").unwrap(),
            vec![action("one", first), action("two", second)],
        );
        assert!(
            matches!(result, Err(ActionError::DuplicateKey(_))),
            "{first}, {second}: {result:?}"
        );
    }
    assert!(
        ActionRegistry::new(
            ScopeId::new("screen").unwrap(),
            vec![action("one", "F1"), action("one", "F2")]
        )
        .is_err()
    );
    // The same chord in distinct scopes is intentionally legal.
    for scope in ["screen", "dialog"] {
        assert!(
            ActionRegistry::new(ScopeId::new(scope).unwrap(), vec![action("open", "Enter")])
                .is_ok()
        );
    }
}

#[test]
fn action_availability_is_read_for_every_invocation() {
    let available = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut definition = action("start", "Ctrl+Enter");
    definition.availability = AvailabilityFn::new({
        let available = available.clone();
        move || {
            if available.load(Ordering::SeqCst) {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "already started".into(),
                }
            }
        }
    });
    definition.run = Handler::from({
        let calls = calls.clone();
        move |()| {
            calls.fetch_add(1, Ordering::SeqCst);
            available.store(false, Ordering::SeqCst);
        }
    });
    let registry = ActionRegistry::new(ScopeId::new("new").unwrap(), vec![definition]).unwrap();
    for _ in 0..2 {
        if let Some(handler) = registry.invocation(&ActionId::new("start").unwrap()) {
            handler(());
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry.presentations()[0].availability,
        Availability::Unavailable {
            reason: "already started".into()
        }
    );
}
