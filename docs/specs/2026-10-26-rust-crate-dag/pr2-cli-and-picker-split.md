# PR 2: split the quota domain out of the CLI, and the session picker out of agent-collaboration

Status: revision 2, ready for planning (independent design review, findings 1–5 accepted and re-checked);  stacked on PR #173 (the crate DAG). Owner, 2026-10-27: "codex router cli is a weird crate too, should be split" and "same with session picker, should be its own crate".

This applies the rule PR #173 established: dependencies point one way, a boundary that must hold is a crate boundary, buckets stay large, and composition lives at the binary. The numbers below come from the cargo-modules 0.28.0 `uses` graph at 7ee273f3. Each crate is folded to its top-level modules, production edges only; test modules are excluded. That graph sees `use` statements, not paths written inside function bodies, so the item lists are a floor.

## Current model

### codex-router-cli (68k lines; 31k production)

| Top-level module | Production lines | Notes |
| --- | --- | --- |
| `presentation` (almost all `presentation::quota`) | 8.8k (+14.4k tests) | iocraft quota UI |
| `quota` | 7.1k (+4.3k tests) | status, refresh service, status JSON, view models |
| `quota_reset` | 4.9k (+3.8k tests) | reset workflow reducer, session supervisor, provider protocol |
| `host_command`, `fleet_command`, `central_command`, `account`, `profile`, `telemetry`, credential workers, parsing, errors | about 9k | the actual CLI |
| `cli_contract_tests` | 14.3k tests | binary-level contract tests |

The quota domain is about 42k of the 68k lines. One production cycle spans seven top-level modules: `account`, `cli_argument_parsing`, `cli_command_errors`, `live`, `presentation`, `quota` and `quota_reset`. It has three causes.

1. **Logic depends on the UI.** `quota` uses `presentation` 60 times:
   - `quota_command_dispatch` and the `quota` module root call `presentation::quota::quota_status_entrypoint::{run_quota_status_view, write_quota_status_view}`;
   - `quota` builds callbacks the UI defines: `CreditUsagePolicySaver`, `CreditUsageRefresher` and `WeeklyQuotaFloorSaver`. These are `Arc<dyn Fn…>` aliases, not traits (`presentation/quota/quota_account_options.rs:20`, `quota_floor_editor.rs:16`). The domain reset-session factory even returns one (`quota_reset/quota_reset_session_composition.rs:9`, `:37`);
   - `quota` reaches the UI's `QuotaStatusViewModel*` types and the `ResetPace*` view types.
2. **The UI and the reset workflow share protocol types.** `presentation::quota` uses `quota_reset` 73 times, for `ResetWorkflowSnapshot`, `ResetSessionIntent`, `ResetIntentSender`, `InspectionTabRequestId` and `WorkflowPhase`, all defined inside `quota_reset::reset_session_supervisor::reset_presentation_protocol` and `reset_workflow_reducer::correlated_effect_contracts`. `quota` and `quota_reset` also use each other: `quota` builds the reset session factory, and the reset loopback harness names `QuotaCommand`.
3. **The argument parser names the commands, and the commands use the parser.** `cli_argument_parsing` names `QuotaCommand` and the other command enums. `quota`, `account` and `live` each use `cli_argument_parsing::ArgumentParser`. `cli_command_errors` wraps every command's error, including `QuotaCommandError`.

The CLI also depends on `agent-collaboration` under its `quota-reset-test-harness` feature.

### agent-collaboration (56k lines)

`sessions`, which is `session_command_dispatch.rs` mounted with `#[path]` and owns the `session_commands` tree (7.9k production lines), and `session_picker` (9.1k production lines plus 13k test lines) form one cycle:
- **picker → sessions, 146 uses:** `session_catalog_records` (47), `session_command_options` (33), `source_ssh_protocol` (31), the router connection registry (20), `source_client_action` (7) and `session_action_selection` (5). The picker also uses the crate-root `picker_runtime_status` and `session_environment::CliContext`.
- **sessions → picker:** `configured_inventory_loader`, `configured_inventory_query`, `session_catalog_records` and `source_client_action` use data types that the picker defines: `SourceInventoryRequest`, `SourceInventoryBinding`, `InventoryPageProgress`, `SourceContinuation`, `SourceInventoryRejection`, `SessionsPickerDataQuery`, `SessionsPickerRequest`, `PickerSourceContext` and `SessionProviderFamily`.

The second edge exists because inventory data types live in the UI module.

## Target

```text
codex-router-tui (exists, iocraft primitives)
        ^                      ^
codex-router-quota-tui   agent-session-picker            UI buckets (iocraft)
        |  \                   |
        v   \                  v
codex-router-quota-service agent-session-inventory        domain buckets (no iocraft)
        ^                      ^
        |                      |
codex-router-cli          agent-collaboration            thin command crates; binaries and composition
(bin codex-router)        (bins agent-collaboration, agent-sessions)
```

Arrows point from a crate to what it depends on. Each command crate depends on its domain crate and its UI crate. No domain crate depends on a UI crate, a command crate, or `agent-collaboration`.

### New crates

| Crate | Owns | Depends on | Must not depend on |
| --- | --- | --- | --- |
| `codex-router-quota-service` | quota status and refresh services, status JSON, the quota view-model data, the reset workflow (reducer, supervisor, provider protocol, credential authority, session composition), and the reset presentation protocol types | the existing router crates `quota` uses today (core, state, selection, auth, secret store, native integration) | iocraft, `codex-router-tui`, the CLI, `agent-collaboration` |
| `codex-router-quota-tui` | `presentation::quota`: components, rendering, editors, the view-model loader, the status view entrypoints | `codex-router-quota-service`, `codex-router-tui`, iocraft | the CLI |
| `agent-session-inventory` | `session_commands`: catalog records, command options, SSH source protocol and executor, router connection registry, inventory loader and query, launch targets. It also takes the moved request/binding/provider-family data types and `picker_runtime_status` | what `session_commands` uses today, minus the picker | iocraft, the picker, `agent-collaboration` |
| `agent-session-picker` | `session_picker`: model, components, background workers, help, layout | `agent-session-inventory`, `codex-router-tui`, iocraft | `agent-collaboration` |

### Breaking the cycles

| Edge | Change | Why this direction |
| --- | --- | --- |
| quota → UI entrypoint | Quota command dispatch (`QuotaCommand` → run) moves into `codex-router-cli`. The CLI calls `codex_router_quota_tui::run_quota_status_view` with handles it builds from `codex-router-quota-service`. | Dispatch is composition: it parses, builds, and picks the renderer. |
| UI-defined callbacks built by quota (`CreditUsagePolicySaver`, `CreditUsageRefresher`, `WeeklyQuotaFloorSaver`) | The aliases stay in `codex-router-quota-tui`, the consumer that needs a stand-in in its tests. The CLI's composition builds the closures over `codex-router-quota-service` services; no new trait. `credit_usage_refresher` leaves the domain reset-session factory interface (`quota_reset_session_composition.rs:37`, used at `:56–60` and `:91–95`). The CLI builds that callback separately, keeping the production/loopback provider-URL pairing it has today, and `create` keeps returning the domain session and ports. | The consumer owns the port; the domain stays UI-free (review finding 1). |
| Quota's use of CLI resources | The `AsyncProviderCredentialResolver` port (`credential_runtime.rs:53`) moves into `codex-router-quota-service`, whose refresh service consumes it (`quota_refresh_service.rs:90`); its implementation stays in the CLI's `credential_runtime`. `QuotaCommandError` splits. The domain errors the services return live in `codex-router-quota-service`. The CLI's command error, which also wraps the resolver-open failure (`quota_command_dispatch.rs:107`, `:154`), stays in the CLI. Opening the secret store (`quota_status_loader.rs:26`) and the background worker's store alias (`quota_background_refresh_worker.rs:172`) become injected inputs: the CLI opens the store and passes the store, availability, resolver and interval. | The consumer owns the port and the domain receives resources rather than opening them (review finding 4). |
| Static table rendering | `quota_status_formatting.rs:39` calls `write_quota_status_view`, which renders an iocraft component. Both the interactive and the static renderer dispatch move to the CLI. The domain produces the view-model data, and the CLI picks the renderer from `codex-router-quota-tui`. | Rendering is UI, choosing it is composition (review finding 4). |
| view-model and projection data used by both | The pure data and pure formatting move into `codex-router-quota-service`, enumerated from consumers. That is `QuotaStatusViewModel*`, `CreditUsageStatus`, `CreditUsageFreshness`, `ResetPace*`, plus quota projection's other UI-owned imports: `QuotaHeaderFacts`, `QuotaPoolCensus`, `QuotaFreshnessCensus`, `QuotaRowStatus` (`quota_status_projection.rs:15`), the body-level `QuotaDisplayWindow` (`:476`), `QuotaAccountStatus` (`quota_status_value_formatting.rs:42`), and `credit_usage_compact_summary`, a pure decimal formatter now housed beside iocraft code (`quota_account_options.rs:639`). The loader and all rendering stay in the UI. Moves go item by item, never by whole module. | Data the logic produces belongs to the logic (review finding 5). |
| UI ↔ reset protocol | The reset presentation protocol (`ResetWorkflowSnapshot`, `ResetSessionIntent`, `ResetIntentSender`, `InspectionTabRequestId`, `WorkflowPhase`) stays in `codex-router-quota-service`, and the UI depends on it. | The workflow owns its protocol; the UI is a client of it. |
| quota ↔ quota_reset inside the domain crate | This becomes an in-crate module edge. The loopback harness's use of `QuotaCommand` goes, because `QuotaCommand` moves to the CLI. The harness takes the reset factory directly, or moves with the dispatch into the CLI's test support. | The harness is test composition. |
| parser ↔ commands | Stays inside `codex-router-cli`, frozen by the ratchet. Optionally, move `ArgumentParser` into its own leaf module that names no command enum, so the commands and the dispatcher both point at it. That breaks the cycle without a crate. | A thin crate's in-crate cycle is cheap. The owner asked for crate splits, not a parser rewrite. |
| inventory → picker data types | `source_inventory_request`, `source_inventory_binding`, `SessionsPickerDataQuery`, `SessionsPickerRoot` (`configured_inventory_query.rs:2`) and `PickerSourceContext` move into `agent-session-inventory`. `SessionsPickerRequest` is the render request (display strings, native arguments, `PickerMachineSourceMode`; `picker_request.rs:9`), so it stays in the picker. Only `SessionProviderFamily` leaves `provider_filter_model.rs`; the rest of that module uses picker state and `codex-router-tui` (`:151`) and stays. | They describe inventory queries and pages, not UI (review finding 5). |
| `CliContext` and `app_server_socket_or_default` reach into inventory and picker | Every inventory-facing API takes named values. Catalog reads take the Codex home and cwd (`session_catalog_query.rs:83`). Source actions take what they capture today (`source_client_action.rs:95`). Launch selection takes the resolved socket and service directory (`session_launch_selection.rs:204`). The picker's client callbacks (`new_session_client_inventory.rs:58`) are built in `agent-collaboration`'s composition. `session_environment` stays in `agent-collaboration` and resolves the values once, keeping the existing debug-home and endpoint-selection decisions. | Hand each part only what it uses. Otherwise inventory needs a forbidden edge to `agent-collaboration` (review finding 2). |
| CLI → `agent-collaboration` (feature `quota-reset-test-harness`) | The CLI calls `agent_collaboration::run_sessions_picker_test_harness` (`codex-router-cli/src/lib.rs:172`). That calls the stored-model terminal fixture, which drives the real command dispatcher and the picker (`picker_entrypoint.rs:38`; `stored_model_terminal_fixture.rs:23–32`, `:109–124`). The cross-command harness dispatch moves into the existing non-publishable `codex-router-quota-reset-test-harness` crate, which may depend on both command crates under their features. The dispatcher fixture stays in `agent-collaboration`, and pure picker storyboards stay in the picker. `codex-router-cli` loses its `agent-collaboration` dependency. Feature forwarding and both existing `--sessions-picker` paths are kept as they run today. | Command crates don't depend on each other. A test harness may sit above them (review finding 3). |

### What stays the same

- Binary names, their packages and their arguments (`codex-router` in `codex-router-cli`; `agent-collaboration` and `agent-sessions` in `agent-collaboration`), the Homebrew formula's executables, release signing identifiers, every CLI and MCP contract, and stored data. This is a crate move, not a behaviour change.
- Test identities: every test keeps its body, mapped old path → new crate and module, as the #173 parity audit did. `cli_contract_tests` stays in the CLI, because it drives the binary.
- Layer policy: `crate_layers.rs` gains the four crates and their allowed edges, and the ratchet baseline is re-keyed for moved modules. Every change must be a shrink or a rename.

### Naming against existing crates (decided 2026-10-27, lane Q's finding)

A `codex-router-quota` crate already exists: a 388-line leaf holding quota snapshots and the refresh worker, used by `codex-router-selection` (`eligibility.rs:4`) and `codex-router-proxy`. Extending it with the moved CLI code would create `codex-router-quota → codex-router-selection → codex-router-quota` and the same cycle through the proxy. Renaming the leaf would edit the proxy, which AGENTS.md keeps out of other work. So the new domain bucket is `codex-router-quota-service`, the application layer over the unchanged leaf, named like `collaboration-service`. It may depend on `codex-router-quota`, `codex-router-selection` and `codex-router-proxy`, and none of them on it.

### Shared test fixtures across the new crates (decided 2026-10-27, lane P's findings)

Inventory fixtures that the picker and `agent-collaboration` tests share live behind a `test-support` feature on `agent-session-inventory` (off by default). Only the dependents' `[dev-dependencies]` enable it, matching `collaboration-service`'s `ApplicationFixture` in #173 and the domain-rust rule. That covers `FaultSourceApi` and its scenarios, `read_complete_source`, `NativeScenario`, `SourceApiFixture` with its recorded listener, and `FakeCollaborationApi`.

Those fixtures launch Node through `codex_router_test_support::node_fixture`. `codex-router-test-support` depends on `codex-router-cli`, so using it from inventory makes a package cycle (inventory → test-support → CLI → agent-collaboration → inventory). `node_fixture` (49 lines, std and serde_json only, used by six crates' tests) moves into a new leaf crate, `node-test-fixture` (`publish = false`, dev-dependency only). Every caller imports it from there, and `codex-router-test-support` drops the module, with no re-export. It is a new crate because the utility has to sit below all its users and no low-level test crate exists. A copy would leave two definitions to drift.

## Alternatives considered

| Option | Gain | Cost | Decision |
| --- | --- | --- | --- |
| **A. One `codex-router-quota-service` crate with logic and UI together, and one `agent-session-picker` crate with inventory and UI** | Two crates instead of four, so fewer manifests and less public surface | The logic → UI inversion and the inventory → picker inversion stay as in-crate cycles. The ratchet freezes them but the compiler doesn't refuse them. The domain crates still pull in iocraft. | Fallback, if review finds the domain/UI seam needs more than the moves above |
| **B. Domain and UI split per area (above)** | The compiler holds both inversions that caused the cycles. Domain crates build and test without iocraft. | Four crates. The three quota UI ports gain CLI adapters. | **Recommended** |
| C. Also split the parser and command errors out of the CLI | Removes the last CLI cycle | A crate for about 1.1k lines, which justify-the-complexity rejects; the ratchet already holds it | Rejected |
| Move-only port variant within B: move `CreditUsagePolicySaver` and the other two traits into `codex-router-quota-service` and implement them there; the UI takes `Arc<dyn …>` | No adapters | Puts UI-shaped interfaces into the domain crate | Fallback, if the adapters turn out to be pure pass-through |

## Proof (for the plan)

- The layer policy test with the new edges passes. A planted upward manifest edge in each new crate (quota → quota-tui, inventory → picker) fails it.
- The ratchet passes against the re-keyed baseline. The two cycles above disappear from the baseline, which is a shrink.
- Test-identity parity per moved test, with the full `cargo nextest run --workspace` and the release-binary contract tests (`cli_contract_tests`, the agent-sessions picker contract tests) passing.
- `cargo tree -p codex-router-quota-service -e normal,build` and `cargo tree -p agent-session-inventory -e normal,build` show no iocraft.
- Real-app proof on Sunclaw, because local executables are stopped on the owner's Mac: `codex-router quota status` (the TUI and `--json`), `agent-sessions` (the picker) and `agent-collaboration sessions list`, run against an isolated debug Router.

## Review record

Independent Sol xhigh design review of revision 1: verdict revise. It raised five findings, all accepted:
1. The reset factory exposes a UI callback.
2. Inventory needs `CliContext`.
3. The harness creates a command-crate edge.
4. Quota uses CLI resources, and static rendering is iocraft.
5. The data moves must go by consumer, not by whole module.

It recommended option B. Revision 2 gives each upward reference an explicit home. The same reviewer's correction re-check of revision 2 (2026-10-27): ready-for-planning, no remaining findings.

## Unverified and open

- Body-level paths. The graph shows `use` edges, so the moves may surface more edges at compile time. The plan's first step is a compile-only spike of the four manifests, with the moves and no logic change.
- Exact file placement within each crate; the plan carries a placement table as in `module-placement.md`.
