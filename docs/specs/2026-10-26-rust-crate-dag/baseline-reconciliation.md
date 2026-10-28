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

## PR 2 (CLI quota split and session picker split)

The baseline was re-keyed from a fresh cargo-modules 0.28.0 scan at 97d7ad79, run on Sunclaw. Against the committed baseline, the ratchet failed only with "baseline SCC … shrank or disappeared" for the two cycles this PR targets. No cycle grew.

| Crate | Before | After | Why |
| --- | --- | --- | --- |
| `agent-collaboration` | {session_picker, sessions} | none | The picker and inventory left the crate. The inventory → picker data edge was cut by moving the data types down. |
| `codex-router-cli` | {account, cli_argument_parsing, cli_command_errors, live, presentation, quota, quota_reset} | {account, cli_argument_parsing, cli_command_errors, live, quota} | Shrink. presentation and quota_reset left the crate. The parser ↔ command cycle stays inside the thin CLI by design (alternative C rejected). |
| `agent-session-inventory` (new) | part of agent-collaboration's {session_picker, sessions} | {picker_data_query, sessions, source_inventory_binding, source_inventory_request} | Moved. This is the same sessions ↔ inventory-data cycle without the picker UI; the folding now shows its four top-level modules. |
| `codex-router-quota-service` (new) | part of the CLI's {quota, …} | {credit_usage_summary, quota, quota_display_model} | Moved. These are pure data and formatting items that the quota logic and its view-model share. |
| `codex-router-quota-tui` (new) | folded inside the CLI's single `presentation` node | {quota_account_options, quota_browse_rendering, quota_provider_filters, quota_reset_detail_content, quota_reset_detail_rendering, responsive_quota_layout} | Became visible, not new. These modules were nested under `presentation`, which the scan folds to one node. As top-level modules of their own crate they show their existing cycles. The ratchet now holds them, and any growth fails. |
| `agent-session-picker`, `node-test-fixture`, `codex-router-tailnet-verification-host` | none | none | The crates are acyclic. |

Follow-up, not in this PR: the quota-tui cycle among its six UI modules is the largest remaining in-crate cycle the split exposed.
