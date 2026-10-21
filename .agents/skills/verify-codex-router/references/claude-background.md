# Native Claude background launch — R15

**Draft/unobserved.** This is an adopted verification journey and test challenge,
not a provider execution grant or working driver. The current assignment remains
verification-skill upkeep. No provider/model launch, funding change or observation
of an unreleased target is authorized. Continue from a separately declared isolated
debug target and its explicit procedure/cleanup grant before driving this journey.

The oracle is Requirements U15, Specification R15, Program Design's "native Claude
background execution" correction and Verification Design U15/R15 under
`docs/specs/2026-10-17-tui-component-system/`, plus the admitted
`tmp/plan-workflows/2026-10-18-tui-component-system-provider-actions.md` S2a.
U16/R16 client/machine/action proof is separately required under S2b and
[client actions](client-actions.md); copy-only rows and this launch correction
cannot close it. The action mapping is admitted; implementation and observation
remain unearned.

## Entry and owners

The actual public entry is `crates/agent-collaboration/src/bin/agent-sessions.rs`
→ `session_environment::run_arguments` → `session_command_dispatch`'s dedicated
Claude path → `session_commands/claude_session_commands.rs`.
`ClaudeLaunchTarget` in `claude_launch_target.rs` constructs native arguments and
routed child environment; `claude_session_commands.rs` executes/reports the child.
The adopted design keeps those owners. Product code and product tests belong to
the Implementer. Future TUI adapters must reuse this owner once U16's mapping is
admitted; do not duplicate background argument rules in a generic dialog.

R15 requires literal `agent-sessions --provider claude --new` to use the installed
native Claude `--bg` contract, returning its actual result/background identity.
New/resume/fork remain background. Prepend the required native background default
and forward every supplied argument unchanged, preserving bytes and order even
for explicit mode aliases and flag-like option values. The installed native CLI
owns its grammar and rejects incompatible print/background modes before native
session/model dispatch. Router propagates that child failure; it does not scan,
deduplicate, remove values or reject before creating the native child.
Prompt/model/effort/working-directory and Router child environment remain
preserved. ACP-managed provider IDs are not native transcript/background
IDs. A shell `&`, detached wrapper PID or invented ID cannot establish this result.

## Separate process, vendor guard and registration proof

| Case | Required observation and wrong behavior it must catch | Current gap |
| --- | --- | --- |
| R15-PUBLIC-NEW | Execute the exact compiled public agent-sessions entry with literal --provider claude --new through a controlled executable boundary. Preserve a causal old-code failure for missing background mode, then observe the corrected invocation and actual child output/exit. | Permanent public-entry source and offered causal RED/5-test GREEN logs checked; frozen delivery executable/run binding still awaited. A direct `launch_arguments()` unit test alone cannot prove entry wiring. |
| R15-RESUME-FORK | Where the adopted native route supports the operation, retain native session identity and background mode, cwd and original args. Existing model/effort/prompt behavior stays. The TUI route must share the same owner. | Offered process tests cover supplied resume/fork argv. U16 adapter implementation and actual native observation are awaited. Never force a provider SessionRef through Codex or reinterpret ACP identity as transcript ID. |
| R15-ARGV-VALUES | The public entry prepends --bg and preserves the complete supplied argv tail exactly, including supplied aliases, option values equal to --bg/--print, required-value arguments, short/long mode spellings and positional text after --. | Source and offered scanner-corruption RED/corrected five-test GREEN checked for their covered values/aliases. Frozen delivery binding and remaining exact variants still need verification. Router must not duplicate the native option-arity table. |
| R15-NATIVE-CONFLICT | On the separately qualified actual native binary, incompatible print/background arguments produce its actual failure before background session/model creation. Forward all inputs and propagate native output/nonzero exit. | Actual vendor negative remains unobserved here. A substitute rejecting those flags proves failure propagation only; no zero-native-child or pre-Router-spawn assertion is owed. |
| R15-FAILURE | Spawn failure and nonzero native child failure remain failures; no success/background ID is fabricated. Child output/actual identity remains attributable to this invocation. | Public-entry source and offered nonzero/spawn-failure cases checked. Real controlled child processes exercise propagation; their synthetic receipt is not a native identity. Frozen delivery binding remains awaited. |
| R15-NATIVE-REGISTRATION | On the separately declared debug target, the actual installed Claude returns while exactly the newly owned native background session is visible through supported `claude agents --json`; correlate its actual identity/result and clean up only that session through qualified supported controls. | Native runtime grant, target, installed binary/help/capability qualification, readiness/identity and cleanup procedure not yet declared. No launch attempted. |

The public process test may replace the external Claude executable to observe
argv/environment/cwd/outputs while keeping parsing, dispatch and actual process
creation real. Label that as process-boundary proof. It cannot demonstrate vendor
registration or its real option grammar/guard. The native observation must use
the real qualified installed client;
list-before/list-after alone is insufficient without this invocation's exact
returned identity and supported session readback. Do not expose routing tokens or
personal transcript contents in evidence.

The Lead's bounded correction was independently checked by reading the installed
native 2.1.296 binary, without executing it. Its background module's `Gt` excludes
indices classified as option values by its own `K` before checking print flags.
New-session `zY` calls that gate before job creation; resume dispatcher `Nn` calls
it before daemon `Te` and session `lXe`. This supports the vendor-owned boundary,
not an observed negative or successful registration. Exact binary SHA and byte
anchors are retained in `tmp/tui-verification/native-guard-source-readback.json`;
requalify after any client change. Do not copy those minified names or arity rules
into Router product code or this verification driver.

## Driving and evidence prerequisites

Before a run, receive the exact compiled agent-sessions and installed Claude
paths/hashes, current source/build declaration, normal isolated debug routing
configuration, approved environment/working directory, allowed native operation,
provider/model/API/funding boundary, deciding readiness/ID readback and exact owned
cleanup. Reading this recipe grants none of them. Check supported controls from
the actual installed help before using them; this draft invents no cleanup command.

Keep the public command, exit, safely redacted child argument/cwd/environment
assertions, actual result/background ID, supported native registration evidence
and owned cleanup settlement. Preserve causal RED separately from post-fix
sensitivity. A harmless controlled executable tests only that boundary; its green
does not authorize a real provider run. Once authorization/target/procedure are
declared, the Lead assigns the lasting native driver paths if needed; the current
TUI wrapper does not drive native background registration.

Return each case as supported/unsupported/unknown with the source/build/client,
actual observation or named gap. No runtime pass or U16 completion is claimed.
