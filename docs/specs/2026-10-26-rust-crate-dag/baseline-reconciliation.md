# Module-cycle baseline reconciliation for the crate split

The ratchet's bless step refuses growth, and a crate split looks like growth: cycles move into packages that didn't exist before. So the baseline was regenerated, and every new strongly connected component (SCC) was checked against main's baseline (`b4881497`) instead of being blessed blindly.

**Method.**
- A fresh scan of the integrated tree, run on a separate Mac with cargo-modules 0.28.0 and the same test and fold code.
- The modules of the three collaboration crates were compared with main's `collaboration-service` by module name, with the crate prefix stripped.
- `service_identity` was treated as `subscription_delivery`, because that subtree was hoisted out of `service_identity`.
- Every new SCC must be a subset of an old SCC, and its edges a subset of the old SCC's edges. Every other package must not grow.

**Result:** 33 SCCs on main and 31 here.

| Package | New SCC | Old SCC on main | Verdict |
|---|---|---|---|
| collaboration-runtime | {app_server_event_forwarding, router_session_app_server} | same, in collaboration-service | re-keyed |
| collaboration-runtime | {provider_acp_event_projection, provider_acp_session_route} | same, in collaboration-service | re-keyed |
| collaboration-runtime | {provider_operation_store, provider_session_record} | same, in collaboration-service | re-keyed |
| collaboration-service | {mail_delivery_runtime, mail_push_delivery, notification_publisher, project_admission, subscription_delivery} | {mail_delivery_runtime, mail_push_delivery, notification_publisher, project_admission, push_record_delivery, service_identity} | shrank: `push_record_delivery` left; `service_identity` became `subscription_delivery` |
| all others | unchanged or smaller | | no growth |

**One accepted new edge:** `application_fixture → subscription_delivery`, entering the mail SCC.
- `application_fixture` is the test-only construction helper. It compiles only with `cfg(test)` or the `collaboration-service/test-support` feature, which consumers enable only in dev-dependencies.
- It is an incoming edge from outside the cycle. It doesn't add a member, and no cycle passes through it.

**A cycle removed rather than baselined:** {codex_session_inventory, collaboration_application, inventory_failures, provider_session_inventory} appeared after the dependency-injection change. Part of it was cargo-modules attributing trait-impl methods to the type's module. It was fixed in source by moving `CollaborationRejection` into its own module, with the inventory types' impls next to those types (`1cb49eda`). The final scan has no such SCC.

The scan is native-target with the default feature resolution described in `architecture/README.md`, and it keeps the documented blind spots for function-body and `super::` paths.
