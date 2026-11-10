//! Strict retired SubscriptionExpiry identity decoding at the upgrade boundary.
use crate::{
    MessageActivityMap, MessageUpgradeError,
    message_rows::{MessageRows, invalid},
};
use collaboration_protocol::{RouterOriginRef, SessionRef};
use message_board::{PostId, ProjectId, SubscriptionGeneration, SubscriptionScope};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum LegacyExpiryOrigin {
    SubscriptionExpiry {
        target: SessionRef,
        scope: LegacySubscriptionScope,
        subscription_generation: SubscriptionGeneration,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum LegacySubscriptionScope {
    Thread { root_message_id: PostId },
    Topic { topic_id: ProjectId },
}

pub(crate) fn translate(
    rows: &mut MessageRows,
    mapping: &MessageActivityMap,
) -> Result<(), MessageUpgradeError> {
    let mut thread_scopes = 0;
    let mut project_scopes = 0;
    for push in &mut rows.pushes {
        if push.origin_kind != "router" || push.kind != "subscription-expiry" {
            continue;
        }
        let encoded = push
            .origin_router_ref
            .as_ref()
            .ok_or_else(|| invalid("push.origin.missing_router_reference"))?;
        let origin: LegacyExpiryOrigin = serde_json::from_str(encoded)
            .map_err(|_| invalid("push.origin.legacy_subscription_scope"))?;
        if serde_json::to_string(&origin)? != *encoded {
            return Err(invalid("push.origin.legacy_canonical_reference"));
        }
        let LegacyExpiryOrigin::SubscriptionExpiry {
            target,
            scope,
            subscription_generation,
        } = origin;
        let scope = match scope {
            LegacySubscriptionScope::Thread { root_message_id } => {
                let Some(thread_id) = mapping.imported_thread(root_message_id) else {
                    thread_scopes += 1;
                    continue;
                };
                SubscriptionScope::Thread { thread_id }
            }
            LegacySubscriptionScope::Topic { topic_id } => {
                let Some(project_id) = mapping.imported_project(topic_id) else {
                    project_scopes += 1;
                    continue;
                };
                SubscriptionScope::Project { project_id }
            }
        };
        push.origin_router_ref = Some(
            RouterOriginRef::SubscriptionExpiry {
                target,
                scope,
                subscription_generation,
            }
            .canonical_string()?,
        );
    }
    if thread_scopes != 0 || project_scopes != 0 {
        return Err(MessageUpgradeError::UnmappedSubscriptionScopes {
            thread_scopes,
            project_scopes,
        });
    }
    Ok(())
}
