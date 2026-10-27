# Old custom driver

The executable, catalogue oracles, fixtures and journey instructions existed to verify provider-native catalogue reconstruction. The custom executable and its supporting files were removed; the shared verification skill itself was restored rather than deleted.


## verify-codex-router/SKILL.md

- **Path:** [.agents/skills/verify-codex-router/SKILL.md](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/.agents/skills/verify-codex-router/SKILL.md)

- **Now:** Present, unchanged from #173.

- **Disposition:** The native-capabilities entry existed at 5f5c49a4 and was removed; the shared skill itself remains.

- **Purpose now:** Indexes established Router verification journeys. The current #173 version contains no native-capabilities registration or old example gate.

- **Purpose at 5f5c49a4 (removed):** Registers the native-capabilities journey in the project verification index and lists Rust example gates.

- **Original reason (recorded in the historical source):** Makes the new J1-J4 verifier discoverable beside the existing ACP journey and defines apparatus checks as separate from runtime proof.

## native-capabilities.md

- **Path:** [.agents/skills/verify-codex-router/references/native-capabilities.md](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/.agents/skills/verify-codex-router/references/native-capabilities.md)

- **Now:** Removed.

- **Historical purpose:** Defines the J1-J4 target, controlled fixtures, qualification, drive sequence, evidence, and limits.

- **Original reason (recorded in the historical source):** Provides the operating contract for proving provider-owned discovery, selection, command invocation, history, and metadata through the installed client and declared candidate.

## apparatus_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/apparatus_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/apparatus_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests paging, parser limits, fixture materialization, provider facts, and recorder reading with temporary evidence.

- **Original reason (reconstructed from code):** Checks that verification helpers reject malformed or misleading inputs; comments expressly limit these tests to apparatus proof.

## bounded_process.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/bounded_process.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/bounded_process.rs)

- **Now:** Removed.

- **Historical purpose:** Runs one child with time and output bounds, stopping only that child on timeout or overflow.

- **Original reason (reconstructed from code):** Keeps CLI reads bounded so an unexpected child cannot hang or consume unbounded output.

## catalogue_oracle.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/catalogue_oracle.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/catalogue_oracle.rs)

- **Now:** Removed.

- **Historical purpose:** Checks J1 skill and plugin discovery against provider-owned files and exports, including explicit unsupported and rejected cases.

- **Original reason (recorded in the historical source):** Prevents the candidate's own response from defining expected catalogue contents and distinguishes unsupported from successful behavior.

## command_options.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/command_options.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/command_options.rs)

- **Now:** Removed.

- **Historical purpose:** Parses the verifier's strict subcommands and options for materialization, J1-J4, and process observations.

- **Original reason (reconstructed from code):** Rejects unknown, repeated, or missing arguments before a journey can touch its declared evidence paths.

## command_reload_probe.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/command_reload_probe.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/command_reload_probe.rs)

- **Now:** Removed.

- **Historical purpose:** Removes the owned second fixture skill, requests the provider's native reload command, and checks successive command lists.

- **Original reason (recorded in the historical source):** Makes replacement observable through a real provider input while restoring exactly the fixture bytes even on failure.

## conversation_oracle.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/conversation_oracle.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/conversation_oracle.rs)

- **Now:** Removed.

- **Historical purpose:** Checks J2-J4 structured selections, rejection, native effects, command lists, display operations, and retained metadata.

- **Original reason (recorded in the historical source):** Separates what the installed client submitted and what the provider did from UI labels or Router receipts.

## conversation_oracle_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/conversation_oracle_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/conversation_oracle_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Exercises J2-J4 oracles with valid and deliberately wrong synthetic evidence.

- **Original reason (reconstructed from code):** Demonstrates sensitivity to dropped or corrupted selection, false native effects, metadata-as-prose, and display-triggered operations; these are not product proof.

## driver_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/driver_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/driver_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests process ownership, bounded child output, strict options, terminal parsing, and PTY driver behavior using harmless local children.

- **Original reason (reconstructed from code):** Validates the driver mechanics independently without exercising a Codex client or Router.

## global_claude_binding.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/global_claude_binding.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/global_claude_binding.rs)

- **Now:** Removed.

- **Historical purpose:** Independently resolves which eligible global Claude executable the Host would select from a supplied PATH.

- **Original reason (reconstructed from code):** Qualifies the actual native executable binding without executing it or changing PATH.

## history_oracle.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/history_oracle.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/history_oracle.rs)

- **Now:** Removed.

- **Historical purpose:** Checks that canonical native command/task history retains the original user facts and identities through read and resume.

- **Original reason (recorded in the historical source):** Detects missing, altered, fabricated, or duplicated history rows while allowing provider echoes as distinct original facts.

## history_oracle_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/history_oracle_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/history_oracle_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests canonical history checks against preserved and corrupted evidence, including dollar signs, Unicode, and newlines.

- **Original reason (reconstructed from code):** Shows the history oracle detects byte-level changes; its fixtures do not establish native history behavior.

## installed_client_terminal.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/installed_client_terminal.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/installed_client_terminal.rs)

- **Now:** Removed.

- **Historical purpose:** Drives an installed Codex TUI in a private PTY and captures bounded screen/input evidence.

- **Original reason (recorded in the historical source):** The selected skill is produced by the client's picker binding and Tab interaction, which a simple prompt argument cannot reproduce.

## journey_run.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/journey_run.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/journey_run.rs)

- **Now:** Removed.

- **Historical purpose:** Shares plan qualification, owned evidence output, Codex invocation, session recorder, and cleanup across journeys.

- **Original reason (reconstructed from code):** Keeps each run tied to one declared candidate and limits process signaling to its own client child.

## metadata_oracle.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/metadata_oracle.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/metadata_oracle.rs)

- **Now:** Removed.

- **Historical purpose:** Checks retained ACP metadata stays outside assistant text while genuine prose survives history views and terminal captures.

- **Original reason (recorded in the historical source):** Carries the merged metadata presentation check into the J4 Rust verifier so the broader native-capabilities changes preserve that behavior.

## metadata_oracle_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/metadata_oracle_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/metadata_oracle_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests metadata and genuine prose checks across live/reopened evidence, wrapped terminal text, and offset provenance.

- **Original reason (reconstructed from code):** Ensures the metadata oracle catches presentation and evidence-provenance mistakes without claiming a client run.

## native_exports.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/native_exports.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/native_exports.rs)

- **Now:** Removed.

- **Historical purpose:** Reads provider-owned CLI exports and the installed client's own recorded session, with bounded output and private evidence writes.

- **Original reason (reconstructed from code):** Supplies independent reference facts for catalogue checks and captures what the client actually sent.

## native_file_facts.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/native_file_facts.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/native_file_facts.rs)

- **Now:** Removed.

- **Historical purpose:** Reads native skill flags, source roles, plugin facts, and marketplace manifests from provider-owned files.

- **Original reason (reconstructed from code):** Lets catalogue expectations come from the native provider's installation and loader inputs rather than the candidate's answer.

## native_fixture.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/native_fixture.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/native_fixture.rs)

- **Now:** Removed.

- **Historical purpose:** Materializes controlled skill files and canaries into a prepared genuine linked worktree, plus an empty comparison directory.

- **Original reason (recorded in the historical source):** Creates a provider-owned test input while refusing overwrites and avoiding installation or global configuration changes.

## native_journeys.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/native_journeys.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/native_journeys.rs)

- **Now:** Removed.

- **Historical purpose:** Implements J1-J4 orchestration over the installed client and Router public surfaces, then passes raw observations to independent oracles.

- **Original reason (recorded in the historical source):** Joins real discovery, selection, command, and history paths while keeping their deciding evidence in separate oracle functions.

## oracle_test_support.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/oracle_test_support.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/oracle_test_support.rs)

- **Now:** Removed.

- **Historical purpose:** Provides reusable controlled evidence builders and temporary filesystem setups for oracle tests.

- **Original reason (reconstructed from code):** Keeps synthetic test inputs consistent and isolated; these builders are test support, not runtime observations.

## ordinary_completion_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/ordinary_completion_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/ordinary_completion_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests how J2 judges the ordinary turn's completion from public provider events.

- **Original reason (recorded in the historical source):** Covers a client display limitation without treating missing terminal rendering as evidence that the provider turn did not complete.

## plugin_catalogue_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/plugin_catalogue_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/plugin_catalogue_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests plugin catalogue, marketplace manifest, plugin details, management rejection, and plugin skill sample oracles.

- **Original reason (reconstructed from code):** Rejects substituted or fabricated plugin evidence and preserves explicit limits for native management support.

## process_identity.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/process_identity.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/process_identity.rs)

- **Now:** Removed.

- **Historical purpose:** Observes process tables and records stable PID, parent, real UID, group, birth, and executable identities without signaling.

- **Original reason (reconstructed from code):** Lets qualification and cleanup evidence distinguish owned processes from foreign or reused PIDs while keeping observation separate from control.

## qualification_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/qualification_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/qualification_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests candidate root/socket/build qualification, Claude binding, config observations, and owned-process checks.

- **Original reason (reconstructed from code):** Fails closed when the verifier is pointed at an unqualified or differently bound target; these are qualification apparatus tests.

## router_evidence.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/router_evidence.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/router_evidence.rs)

- **Now:** Removed.

- **Historical purpose:** Reads app-server replies, bounded public event observations, Router operation rows, and health from the candidate.

- **Original reason (reconstructed from code):** Collects public/read-only evidence for the oracles and avoids treating partial event pages or health readiness as behavioral proof.

## router_queue_observation.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/router_queue_observation.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/router_queue_observation.rs)

- **Now:** Removed.

- **Historical purpose:** Reads the declared Session's existing delivery queue through the Host's private ACP channel and queue-list route.

- **Original reason (reconstructed from code):** Allows negative selection/queue checks without adding, cancelling, or reordering queued inputs.

## skill_catalogue_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/skill_catalogue_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/skill_catalogue_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests skill catalogue membership, source role, disabled status, workspace scope, and explicit unsupported outcomes.

- **Original reason (reconstructed from code):** Makes wrong path, borrowed workspace, false disabled facts, and malformed source evidence fail at the oracle boundary.

## skill_source_membership_tests.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/skill_source_membership_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/skill_source_membership_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tests Claude 2.1.310 native source membership for immediate skill entries and recursive legacy command Markdown, including symlinks.

- **Original reason (reconstructed from code):** Binds catalogue acceptance to the provider loader's actual source roles instead of treating physical containment as sufficient.

## target_qualification.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/target_qualification.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/target_qualification.rs)

- **Now:** Removed.

- **Historical purpose:** Refuses a run unless the declared candidate source, binaries, identities, endpoint, fixture, and provider binding qualify.

- **Original reason (recorded in the historical source):** Keeps live observations attached to the exact Lead-started candidate and rejects mismatched evidence targets.

## verification_models.rs

- **Path:** [crates/codex-router-host/examples/provider_native_capability_verification/verification_models.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/provider_native_capability_verification/verification_models.rs)

- **Now:** Removed.

- **Historical purpose:** Defines strict plan, fixture, provider, process, RPC, event, history, and observation data views consumed by the verifier.

- **Original reason (reconstructed from code):** Constrains evidence decoding to the declared target and used fields while preserving raw replies as authoritative records.

## verify-provider-native-capabilities.rs

- **Path:** [crates/codex-router-host/examples/verify-provider-native-capabilities.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/examples/verify-provider-native-capabilities.rs)

- **Now:** Removed.

- **Historical purpose:** Cargo example executable dispatching materialize, J1-J4, process-capture, and process-check commands; embeds its driver and oracle test modules.

- **Original reason (inferred from structure):** Provides one runnable verifier target and keeps unit sensitivity tests alongside the implementation they exercise.

## router-native-arguments/SKILL.md

- **Path:** [scripts/verification/fixtures/provider_native_capabilities/router-native-arguments/SKILL.md](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/scripts/verification/fixtures/provider_native_capabilities/router-native-arguments/SKILL.md)

- **Now:** Removed.

- **Historical purpose:** Defines an argument-sensitive native skill that returns a body-only canary while preserving dollar signs, Unicode, and newlines.

- **Original reason (recorded in the historical source):** Separates actual application of the selected skill body from an echoed command label or fixed marker.

## router-native-second/SKILL.md

- **Path:** [scripts/verification/fixtures/provider_native_capabilities/router-native-second/SKILL.md](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/scripts/verification/fixtures/provider_native_capabilities/router-native-second/SKILL.md)

- **Now:** Removed.

- **Historical purpose:** Defines a second controlled skill used to form rejected multi-skill selections and a removable command-list entry.

- **Original reason (recorded in the historical source):** Supports negative selection and replacement cases with a known native fixture identity.

## native-capabilities.sh

- **Path:** [scripts/verification/native-capabilities.sh](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/scripts/verification/native-capabilities.sh)

- **Now:** Removed.

- **Historical purpose:** Changes to repository root and invokes the locked Rust example target with its arguments.

- **Original reason (reconstructed from code):** Gives the verifier a stable documented command without installing or replacing a binary.
