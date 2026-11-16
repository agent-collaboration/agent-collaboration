# Schema audit: complete outside-edge recordings

This journey observes the debug Router's live exchanges with Claude and an MCP caller, and checks each captured message against its owner's definition. Managed-Codex wire evidence comes from the N10/N8 fake-server captures; its live edge is a named gap. Capture the connection first; select case messages afterwards. A successfully checked subset does not prove a complete capture.

**Status: draft; the complete live matrix remains unverified.** Driver entry contracts are described below; require their frozen delivered version and real composition checks before using the route as proof. A partial live success does not qualify the remaining actions. Do not infer launch flags or use an unrelated driver's defaults to fill those gaps.

Adopted reference: D0 [Specification](../../../../docs/specs/2026-11-13-protocol-contracts/d0/specification.html) R36/R42/R50–R54, [Program Design](../../../../docs/specs/2026-11-13-protocol-contracts/d0/program-design.html) owners/check/capture/live, and [Verification Design](../../../../docs/specs/2026-11-13-protocol-contracts/d0/verification.html) OB4. The same journey is lasting for D1 and D2. Keep task results outside this skill.

## Declare the target and the run

Require one frozen declaration containing the source commit/worktree, signed debug Router and Host paths/hashes, client executables/versions, debug root, service endpoint, private evidence directory, allocation deadline, configuration and model settings. Record actual identities and compare them before any run. A changing build or configuration needs a new declaration before dependent observation.

Use only the declared debug Router. Model input is limited to the approved Haiku 5.5 and `gpt-6-luna` at low effort. Confirm the actual effective setting before each input, including after resume. The Claude 0.81.0 advertisement can expose Haiku 5.5 under model ID `haiku` and reject the literal `claude-haiku-5-5`; use an alias only after its actual advertised label and effective session settings establish the approved model. Never substitute another model family after a refusal. No login, account switch, production process replacement or home/config write follows from this recipe. Normal Codex home stays normal; its other conversations are outside the capture.

Require exclusive use of the declared debug Host and a confirmed allocation before starting. A slot's start time is not confirmation that another lane is finished. When foreign traffic arrives, stop new submissions, retain only demonstrably owned captures, report the contamination, and perform owned cleanup. Do not query foreign session histories to diagnose it. Resume only after the Lead confirms the conflicting lane is free and declares the next target/allocation.

Use an existing owner debug root in place only when the run explicitly authorizes it. Do not copy its state, OAuth material or secrets into an apparently isolated root. Inventory pending jobs without reading message bodies before launch, and keep owner state separate from the private recording directory. Recheck artifact hashes and signatures before every launch: Cargo tests/examples can relink top-level binaries after an earlier build receipt.

Use the main skill's process ownership, signing, configuration guards and cleanup. Its generic `owned-runtime.ts` launch is not by itself a qualified-Claude route: its `hostCommand` passes a Node executable plus adapter arguments, while `claude_launch.rs::qualify_launch` requires empty configured arguments. Require the delivered schema-audit plan to show the actual qualified launch and recorder composition.

The adopted managed-Codex boundary is settled: build no live tee plan. `crates/codex-native-integration/src/app_server_launch.rs:77–80` uses the configured socket for `--listen`; `crates/codex-router-host/src/lifecycle_owner/collaboration_lifecycle.rs:75` uses it for the backend connection; `crates/codex-router-host/src/lifecycle_owner.rs:319–326` refuses an already-listening endpoint. Keep `no live wire coverage` with these anchors in the report, and require the checked N10/N8 fake-server captures. Product changes, replacing the real Codex install and a fresh Codex home are outside this journey. These local captures supply no live-provider credit.

The delivered plan must bind these entry contracts to its exact source, signed artifacts and fresh output paths:

| Component      | Home and required contract                                                                                                                                                                            |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude preload | `verification/observations/providers/acp/acp-stdio-recorder.mjs`: invocation-local provider environment, complete JSON-RPC capture, no qualification-probe pollution, explicit recording destination. |
| MCP caller     | `verification/observations/providers/mcp/mcp-live-record.ts`: declared debug endpoint, actual request/reply capture, scoped tool arguments, readback and cleanup.                                     |
| Shared checker | Test-support example `check_recording`: actual edge selector/file contract, complete per-message results and corrupt-capture rejection. The same checker serves fixture and live recordings.          |
| Problem list   | `verification/tooling/schema-audit/schema-audit-report.ts`: consumes actual checker results, inventory/coverage ledger and tap outcomes; reconciles totals and writes the findings.                   |

Before launch, read each delivered script's source/help and the actual run plan. Missing source, missing output contract, unavailable permissions or an undeclared target is a named setup gap. Do not start a partial live run while guessing the rest.

### Entry contracts

Run from the repository root. Bind `ACP_RECORDING` and `MCP_RECORDING` to the actual captured files, `MCP_PLAN` to the reviewed JSON plan, `TAP_DIR` to the fresh external part-test capture directory, `LIVE_DIR` to the directory holding this run's derived results, and `AUDIT_OUTPUT` to the chosen report file. Use the declared Node 26.13 runtime. These commands are the delivered source interfaces; a command returning zero does not close the observations below.

```sh
fnm exec --using v26.13.0 node verification/observations/providers/mcp/mcp-live-record.ts "$MCP_PLAN"
cargo run -p codex-router-test-support --example check_recording -- acp-agent "$ACP_RECORDING" --out "$LIVE_DIR/claude.check.json"
cargo run -p codex-router-test-support --example check_recording -- mcp "$MCP_RECORDING" --out "$LIVE_DIR/mcp.check.json"
fnm exec --using v26.13.0 node verification/tooling/schema-audit/schema-audit-report.ts "$TAP_DIR" --live "$LIVE_DIR" --out "$AUDIT_OUTPUT"
```

`check_recording` also accepts `cli-codex`, `managed-codex`, `acp-served-n8` and `acp-served-n9` for the corresponding declared recordings. Its default output is `<recording>.check.json`; malformed lines, sequence gaps, incompatible message/redacted fields or missing initialization reject with exit 2. Inspect result counts and unanswered requests, including their transfer to the final report; exit zero alone can still contain violations or incomplete coverage.

Every line is `{seq, connection, direction, message}`, with directions `clientToServer` / `serverToClient` (the legacy tee aliases are also accepted). Privacy exclusions use `redacted` with method/thread-id hash instead of `message`. The checker's results keep those exclusions separate from checked frames. Preserve original kept captures and result files; choose fresh destinations because a checker result path may be overwritten by its CLI.

For Claude, set `NODE_OPTIONS` to import the absolute `acp-stdio-recorder.mjs` path and set `CODEX_ROUTER_D0_ACP_RECORDING_DIR` to a fresh absolute directory on the **debug Host process invocation**, so its provider children inherit them. Configure the official 0.81.0 package's absolute `dist/index.js` path with empty arguments. Its pnpm `.bin/claude-agent-acp` shell shim need not qualify as that entry; do not substitute the generic Node-plus-adapter-argument launch. The recorder starts `acp-<pid>.jsonl` only on an incoming ACP initialization with numeric protocol version, leaving non-ACP qualification/MCP children unrecorded. Treat any corresponding `acp-<pid>.failure` as failed capture; check for it after startup and final settlement before using the JSONL prefix as evidence.

The MCP plan has `endpoint`, fresh absolute `recordingPath`, `sessionModel`, and `steps` containing `name`, `tool` and `arguments`. The three required step names are `sessionCommand`, `acpCarrier` and `nativeControl`; verify all three occur in the actual plan. The caller refuses non-loopback HTTP endpoints, known production ports, disallowed plan model names and existing recording paths. `sessionModel` is a declaration, not inspection of the actual target model/effort; verify effective settings before a call that can create or prompt. Its `<recording>.steps.json` records unadvertised tools as not exercised. Keep that sidecar in `LIVE_DIR` so the audit reads it, alongside the checker output.

Inspect each returned tool outcome as well as the step label: a driver may record an invoked step as exercised even when the product returns `notSubmitted`. An idle-session steer refusal proves that refusal, not active steering. Keep the full required matrix explicitly unverified until its successful interactions are recorded.

## Prove composition without model input

Run the delivered model-free composition checks first, retaining each command, exit status, input/capture hash and output path.

1. **Qualified Claude with preload.** `prepare_claude_launch` in `crates/acp-provider-adapters/src/claude_launch.rs` retains the launch environment when replacing executable/arguments. Its import probe inherits that environment too. The recorder must leave probe output intact and produce no ACP recording for the probe. Confirm the actual qualified child argv (`--eval` wrapper and pinned library) plus the captured complete `initialize` request/reply and package version. Retain this process identity evidence; an available endpoint alone does not prove qualification. Record a missing literal `qualified Claude ACP metadata launch` log line as a named limit, separately from the accepted argv/initialize evidence. The CLI exports tracing through OTLP rather than formatting logs to stdout: if a commission additionally requires the literal event, declare a fresh `CODEX_ROUTER_OBSERVABILITY_MARKER` and its existing local exporter destination before launch, then query only that run through the observability skill. Do not start a stack or query unrelated logs from this recipe, or rerun qualified behavior merely to replace the missing log observation.
2. **Complete capture.** Include initialization, session setup, replies, notifications and unexpected extra frames from the first frame on each connection. Reconcile captured and classified counts. A controlled extra frame outside the selected case method must still enter the checker. Split UTF-8 bytes across chunks must retain the original message; an unterminated final line is incomplete capture. Preserve the exact raw value before timestamp/id normalization; stable expected-output copies remain separate.
3. **Privacy before write.** Use harmless synthetic other-thread markers for `thread/read`, `thread/turns/list`, `thread/items/list`, `thread/list` and `thread/search`. Exclude the request and correlated reply before either is written. Prove correlation with equal IDs on different connections and requests in opposite directions. Include allowed owned traffic as a positive control and preserve relay forwarding. The test must not read another real conversation. Retain exclusion method/hash/count evidence, not the excluded payload.
4. **Checker rejection.** A sequence gap, unparseable line, truncated exchange or missing initialization cannot yield a complete/conforming recording. Orphan/unanswered replies and missing negotiation must remain incomplete coverage. Every retained frame gets a result or a named gap; no label-only record counts as a seen kind.
5. **MCP composition.** The actual caller's initialization, advertised schemas and controlled calls must enter the shared checker. Its input/output bodies and JSON-RPC envelopes use their respective definitions. Verify advertised Codex definitions against the independent pin; validating a schema against itself cannot establish that equality.

When composition fails, retain the failure and stop only its dependent live drive. A preload that defeats qualification is a driver conflict, not evidence of a provider defect. The managed-Codex gap is already settled; do not reopen tee placement or create a substitute route during this run.

## Drive the declared live edges

Use one owned debug instance for the batch. Check its readiness and identity before the first action and after a failed action. Keep request, operation, input, turn, session and connection identities distinct. A returned acknowledgement is not evidence of the later protocol exchange.

| Edge          | Actions and deciding observation                                                                                                                                                                                                                                                                                                                                     |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude ACP    | Create an owned session on the declared Haiku model; send one controlled prompt; steer; change a supported configuration; obtain one genuine permission request. Keep each action's prerequisite from the actual initialization/session capability data. Confirm each request/reply or notification in the complete preload capture and the shared checker's result. |
| Managed Codex | Verify that the report lists `no live wire coverage` with the source anchors above, and that the complete N10/N8 fake-server captures are present and checked. Run no live tee journey for this edge. Keep local fake-server evidence separate from live Claude/MCP results.                                                                                         |
| MCP           | The caller records `initialize`, `tools/list`, one session-command call, one ACP-carrier call and one native-control call against the declared debug Host. Read the schemas actually advertised on that Host; record the real envelopes and their scoped arguments/output. The N11 test-only tap supplies no live evidence.                                          |

A live action runs only when its prerequisite is observed. A missing capability is `not exercised` with its owner and closure condition, never a silently skipped action or a synthetic replacement. Keep support separate from shape: a well-shaped request may be unsupported on that connection; do not loosen its pinned schema.

Keep the named MCP examples distinct from broader category coverage: successful settings and native rename calls do not replace specifically commissioned `conversation_cancel` and `turn_interrupt` observations. Cancel an actual running owned prompt and read both the cancel operation and target prompt to terminal settlement. For interrupt, retain its actual accepted turn ID and generation, immediately observe that owned session as active, then send the guarded interrupt for that turn. Acceptance alone is not the active-state prerequisite. A transient metadata read failure or an idle readback leaves the step unobserved; preserve it and never invent a turn ID. Stop after a real refusal or successful interrupt according to the commissioned scope.

### Permission setup

Use a fresh owned requester and an actual reachable test Session as its Approver, with explicit actor attribution. The identity, offered-option validation and harmless private marker mechanics are taught in [typed provider callbacks](typed-callbacks.md); the schema-audit live plan must provide the actual permission drive. Its existing combined Question/Approval script is not automatically a permission-only D0 entry, and D0 adds no mandatory Question or owning-reopen journey.

A production SessionRef cannot be relabeled as a debug caller. If a fresh debug service has no valid local controller, return that bootstrap gap to the Lead. With explicit authorization, use the supported OS-owner Human creator route to create a controller, then use real debug Session actors to create the Approver and requester. Human metadata creation is not Human approval authority. Keep the requester's Approver an actual reachable Session and attribute its decision to that Session. Confirm invocation capability before supplying prompt options: an owned session's creation workdir does not imply the provider accepts a prompt `cwd` override.

Before input, fix the harmless command and private destination in the run plan. On genuine emission, match the actual request and exact offered allow-once option. Never invent a Human identity, answer with a fabricated option ID or give a session/persistent grant. An unexpected command, option or target ends that step without granting it. Record provider callback/tool/turn settlement and the scoped effect when applicable, so an interaction acknowledgement is not confused with consumption.

No permission emission is `not exercised` or a missing observation, with its actual reason. A fixture-generated permission can qualify composition; it does not satisfy the live action.

## Read results and settle the run

Check each complete live recording using the delivered `check_recording` entry and feed those actual results to the schema-audit report. Retain the original kept recording separately from derived results. Verify that:

- each retained connection initializes, every sequence/frame is accounted for and every reply uses its own request's method;
- unanswered ledger requests reach the single problem list as coverage gaps naming their connection and request;
- unseen kinds are counted by definition owner, sending role and method, so a same-named method from another protocol or direction supplies no coverage;
- exact raw wire/body values are checked; typed inputs/outcomes are counted separately;
- owner selection distinguishes standard ACP, Claude upstream extras, Router-owned extensions and Cursor's absent authoritative definition using the actual edge plus method. Claude 0.81.0 `_auth/status_update` has authority in packaged `dist/auth-status.d.ts` and its emitted notification; a same-named Cursor method must not erase that Claude authority;
- all violations carry record identity, schema/instance path, rule and the rule's own detail discriminator; two missing required properties remain two defects, and fixing either names the one removed;
- exact known-defect sets reject additions, removals and changed failures independently;
- Router-owned model metadata uses the real Zod verdict and Rust decoder outcome on the same raw value, comparing retained model IDs, choices and defaults through public getters; preserve valid Unknown separately from malformed/version fallback. Add no encoder. Round trips apply only to the existing bidirectional session-profile codecs;
- each injected record has its own intent: only its declared invalid violation is exempt while the product rejects it; another violation on that record or another product frame remains an ordinary finding, and apparatus records stay distinct;
- every finding and unexercised/coverage gap has its place and fixing owner, and the report totals reconcile to actual inputs;
- the managed-Codex live gap remains explicit beside the checked N10/N8 fake-server evidence;
- no excluded thread content occurs in kept requests, replies, stderr or secondary evidence.

Close only owned test sessions after settlement, retaining their exact terminal operations. An admitted or may-have-dispatched close is not terminal cleanup. If read-only SQLite supplies the final readback, include its existing WAL: immutable mode can silently omit recent operations. Persistent provider metadata records alone do not mean a provider process is alive. Stop only the recorded processes/listeners the plan owns, then apply the main skill's configuration and lifecycle settlement checks. Preserve evidence and failed/uncertain operation IDs. Shared controllers and production processes stay untouched.

Use `readSavedCodexHome` from `runtime-configuration.ts`, which calls `decodeSavedCodexHomeSnapshot` at the owning reader boundary. The exact decoder retains fingerprint integers as BigInt and normalizes only nonnegative safe section occurrence counts to Number; unsafe counters are rejected. Treat any decode failure as an apparatus failure, not unchanged-config proof, and preserve it. Do not reuse an observation-only conversion from an earlier run. Keep `configurationGuardPassed` separate from `configurationUnchanged`: permitted external section changes can pass the guard while literal whole-file equality is false. Retain those section hashes/classifications with writer unattributed, never restore the files or coerce fingerprint values to obtain a pass.

Return the observed target and configuration, actual capabilities/settings, each action and result, checker/report commands with exits and counts, findings by owner, remaining gaps, privacy sensitivity evidence and owned cleanup. Mark model-free fixtures, local component tests and live-provider observations separately. Until a real target has passed the composition and scoped drives, return this journey as draft/blocked with the missing prerequisite; writing the recipe or passing static checks proves no live behavior.
