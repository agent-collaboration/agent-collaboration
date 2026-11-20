//! Pure permanent actor/root ownership cases; no client/service/store or runtime is invoked.
use super::{CleanupIntent, MatrixCleanupPlan, OwnedMatrixResources, OwnershipError};
use crate::proof_context::ProofResult;
use collaboration_client::board::{
    EndpointId, Identity, ServiceId, SessionEndpointRef, SessionId, SessionRef, ThreadId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Actor {
    Sender,
    Reader,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Root {
    First,
    Second,
    Third,
}
fn actor(value: Actor) -> ProofResult<Identity> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from(
                match value {
                    Actor::Sender => "matrix-sender",
                    Actor::Reader => "matrix-reader",
                }
                .to_owned(),
            )?,
        },
    })
}
fn root_id(value: Root) -> ProofResult<ThreadId> {
    Ok(ThreadId::try_from(
        match value {
            Root::First => "019f0000-0000-7000-8000-000000000021",
            Root::Second => "019f0000-0000-7000-8000-000000000022",
            Root::Third => "019f0000-0000-7000-8000-000000000023",
        }
        .to_owned(),
    )?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Intent {
    Unsubscribe(Actor, Root),
    Unwatch(Actor, Root),
}
fn pair_view(actor_value: &Identity, root: &ThreadId) -> ProofResult<(Actor, Root)> {
    let actor_label = if *actor_value == actor(Actor::Sender)? {
        Actor::Sender
    } else if *actor_value == actor(Actor::Reader)? {
        Actor::Reader
    } else {
        return Err("cleanup intent changed its actual actor identity".into());
    };
    let root_label = if *root == root_id(Root::First)? {
        Root::First
    } else if *root == root_id(Root::Second)? {
        Root::Second
    } else {
        return Err("cleanup intent changed its actual root identity".into());
    };
    Ok((actor_label, root_label))
}
fn view(plan: &MatrixCleanupPlan) -> ProofResult<Vec<Intent>> {
    let mut intents = Vec::new();
    for action in &plan.intents {
        intents.push(match action {
            CleanupIntent::Unsubscribe {
                actor,
                root_message_id,
            } => {
                let (actor, root) = pair_view(actor, root_message_id)?;
                Intent::Unsubscribe(actor, root)
            }
            CleanupIntent::Unwatch {
                actor,
                root_message_id,
            } => {
                let (actor, root) = pair_view(actor, root_message_id)?;
                Intent::Unwatch(actor, root)
            }
        });
    }
    let historical_pairs = plan
        .history
        .actor_roots
        .iter()
        .map(|pair| pair_view(&pair.actor, &pair.root_message_id))
        .collect::<ProofResult<Vec<_>>>()?;
    let unsubscribe_pairs = intents
        .iter()
        .filter_map(|intent| match intent {
            Intent::Unsubscribe(actor, root) => Some((*actor, *root)),
            Intent::Unwatch(..) => None,
        })
        .collect::<Vec<_>>();
    if historical_pairs != unsubscribe_pairs || historical_pairs.len() * 2 != intents.len() {
        return Err("actor/root history does not match exact cleanup-pair ownership".into());
    }
    Ok(intents)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Prefix {
    SenderFirstBeforeCreate,
    ReaderFirstBeforeJoin,
    FirstPolicyBeforeSecondRoot,
    SenderSecondBeforeCreate,
    ReaderSecondBeforeJoin,
    SecondPolicyRegistered,
}
fn at_prefix(prefix: Prefix) -> ProofResult<OwnedMatrixResources> {
    let mut owned = OwnedMatrixResources::default();
    owned.register_actor_root(actor(Actor::Sender)?, root_id(Root::First)?)?;
    if prefix == Prefix::SenderFirstBeforeCreate {
        return Ok(owned);
    }
    owned.register_actor_root(actor(Actor::Reader)?, root_id(Root::First)?)?;
    if prefix == Prefix::ReaderFirstBeforeJoin {
        return Ok(owned);
    }
    owned.register_actor_root(actor(Actor::Reader)?, root_id(Root::First)?)?;
    if prefix == Prefix::FirstPolicyBeforeSecondRoot {
        return Ok(owned);
    }
    owned.register_actor_root(actor(Actor::Sender)?, root_id(Root::Second)?)?;
    if prefix == Prefix::SenderSecondBeforeCreate {
        return Ok(owned);
    }
    owned.register_actor_root(actor(Actor::Reader)?, root_id(Root::Second)?)?;
    if prefix == Prefix::ReaderSecondBeforeJoin {
        return Ok(owned);
    }
    owned.register_actor_root(actor(Actor::Reader)?, root_id(Root::Second)?)?;
    Ok(owned)
}

#[test]
fn prefix_failures_preserve_literal_owned_intents_and_history() -> ProofResult<()> {
    use Actor::{Reader, Sender};
    use Intent::{Unsubscribe, Unwatch};
    use Prefix::*;
    use Root::{First, Second};
    struct Row {
        prefix: Prefix,
        intents: &'static [Intent],
    }
    // Literal expected rows follow actual C order: pairs in registration order.
    let rows = [
        Row {
            prefix: SenderFirstBeforeCreate,
            intents: &[Unsubscribe(Sender, First), Unwatch(Sender, First)],
        },
        Row {
            prefix: ReaderFirstBeforeJoin,
            intents: &[
                Unsubscribe(Sender, First),
                Unwatch(Sender, First),
                Unsubscribe(Reader, First),
                Unwatch(Reader, First),
            ],
        },
        Row {
            prefix: FirstPolicyBeforeSecondRoot,
            intents: &[
                Unsubscribe(Sender, First),
                Unwatch(Sender, First),
                Unsubscribe(Reader, First),
                Unwatch(Reader, First),
            ],
        },
        Row {
            prefix: SenderSecondBeforeCreate,
            intents: &[
                Unsubscribe(Sender, First),
                Unwatch(Sender, First),
                Unsubscribe(Reader, First),
                Unwatch(Reader, First),
                Unsubscribe(Sender, Second),
                Unwatch(Sender, Second),
            ],
        },
        Row {
            prefix: ReaderSecondBeforeJoin,
            intents: &[
                Unsubscribe(Sender, First),
                Unwatch(Sender, First),
                Unsubscribe(Reader, First),
                Unwatch(Reader, First),
                Unsubscribe(Sender, Second),
                Unwatch(Sender, Second),
                Unsubscribe(Reader, Second),
                Unwatch(Reader, Second),
            ],
        },
        Row {
            prefix: SecondPolicyRegistered,
            intents: &[
                Unsubscribe(Sender, First),
                Unwatch(Sender, First),
                Unsubscribe(Reader, First),
                Unwatch(Reader, First),
                Unsubscribe(Sender, Second),
                Unwatch(Sender, Second),
                Unsubscribe(Reader, Second),
                Unwatch(Reader, Second),
            ],
        },
    ];
    for row in rows {
        let plan = at_prefix(row.prefix)?.into_cleanup_plan();
        if view(&plan)? != row.intents {
            return Err(format!(
                "ownership prefix {:?} differs from literal expectation",
                row.prefix
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn empty_owner_has_no_actions_or_history() -> ProofResult<()> {
    let plan = OwnedMatrixResources::default().into_cleanup_plan();
    if !view(&plan)?.is_empty() || !plan.history.actor_roots.is_empty() {
        return Err("empty ownership produced cleanup or historical state".into());
    }
    Ok(())
}

#[test]
fn identical_policy_pairs_dedupe_and_fifth_distinct_pair_is_refused() -> ProofResult<()> {
    let mut owned = at_prefix(Prefix::ReaderSecondBeforeJoin)?;
    owned.register_actor_root(actor(Actor::Sender)?, root_id(Root::First)?)?;
    owned.register_actor_root(actor(Actor::Reader)?, root_id(Root::Second)?)?;
    if owned.register_actor_root(actor(Actor::Sender)?, root_id(Root::Third)?)
        != Err(OwnershipError::ActorRootLimitReached)
    {
        return Err("fifth distinct actor/root pair was not refused before dispatch".into());
    }
    let plan = owned.into_cleanup_plan();
    use Actor::{Reader, Sender};
    use Intent::{Unsubscribe, Unwatch};
    use Root::{First, Second};
    let expected = vec![
        Unsubscribe(Sender, First),
        Unwatch(Sender, First),
        Unsubscribe(Reader, First),
        Unwatch(Reader, First),
        Unsubscribe(Sender, Second),
        Unwatch(Sender, Second),
        Unsubscribe(Reader, Second),
        Unwatch(Reader, Second),
    ];
    if view(&plan)? != expected || plan.history.actor_roots.len() != 4 {
        return Err("pair deduplication or refusal changed actual four-pair ownership".into());
    }
    Ok(())
}

#[test]
fn full_owner_has_exact_eight_actor_root_actions() -> ProofResult<()> {
    let plan = at_prefix(Prefix::SecondPolicyRegistered)?.into_cleanup_plan();
    let actual = view(&plan)?;
    use Actor::{Reader, Sender};
    use Intent::{Unsubscribe, Unwatch};
    use Root::{First, Second};
    let expected = vec![
        Unsubscribe(Sender, First),
        Unwatch(Sender, First),
        Unsubscribe(Reader, First),
        Unwatch(Reader, First),
        Unsubscribe(Sender, Second),
        Unwatch(Sender, Second),
        Unsubscribe(Reader, Second),
        Unwatch(Reader, Second),
    ];
    if actual != expected || actual.len() != 8 || plan.history.actor_roots.len() != 4 {
        return Err("full owned actor/root pairs exceeded exact cleanup bounds".into());
    }
    Ok(())
}
