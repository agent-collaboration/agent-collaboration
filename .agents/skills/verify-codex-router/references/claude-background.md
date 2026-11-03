# Native Claude background New: start and attach

**Draft/unobserved on the corrected target.** A background New starts one native
job and attaches the terminal to it. Detaching returns control while that job
keeps running, and restores termios. The owner corrected this on 2026-10-30:
"it's supposed to attach to it or launch it". The correction is Requirements U12
at design revision `74f1fb5b86f18c4cfee360a59a930a1d4e91bc76` under
`docs/specs/2026-10-29-tui-component-system/`. It supersedes the earlier statement
that background New leaves the user in the picker.

The later Open-only correction is R13/U12 at
`014e25e0851f1bef48b02a0d80e5167ec92fd7c2`: stopped Claude Details offers
**Open**, which resumes the same native session with its saved settings and
lands inside it. **Resume in background must be absent**; seeing it fails B and
F. Background remains a New choice, not a separate stopped-session action.

The old receipt "Started in background… Open it from the list" followed by a
return without attachment is a **wrong outcome**. It cannot be reused as positive
proof of this case. Existing 10-17/U15/R15 public-entry, original-argument,
provider-owned guard and exact identity obligations still apply. The product
correction comes from its separately owned worker before S6.

This recipe grants no provider/model launch, funding change, public Host, personal
session operation or cleanup. Obtain the exact declared target and allowed-effects
packet before runtime work. S9, not this authoring pass, drives the corrected
compiled entry. See [client actions](client-actions.md) for the separate actual
Human/client/machine/identity obligations.

## Entry and ownership

Public entry: `crates/agent-collaboration/src/bin/agent-sessions.rs` →
`session_environment::run_arguments` → `session_command_dispatch`'s Claude path →
`session_commands/claude_session_commands.rs`. `ClaudeLaunchTarget` in
`claude_launch_target.rs` owns native argv/routed environment; the command owner
executes and reports the native result. TUI New delegates to that same owner.
Do not implement mode handling, attach or detach in verification apparatus.

For the TUI case, select Claude Code and Run Background, or use the menu's
"New background session…". Foreground remains a separate supported choice.
A separately declared public-entry case uses the actual supported public command;
qualify its mode arguments from the corrected source/help rather than inventing
flags. The established `agent-sessions --provider claude --new` process case keeps
its literal argv until its owning correction explicitly changes that contract.

Native IDs, Router SessionRefs and ACP-managed IDs retain their own meanings.
A shell `&`, wrapper PID or invented ID never proves background registration or
attachment. Preserve model, effort, prompt, cwd, routed environment and supplied
argument bytes/order. The native client owns its option grammar and incompatible
print/background rejection; Router propagates its actual result instead of
scanning or rewriting the caller's argv.

## Deciding journey

1. Bind the corrected source/build, native executable/hash/version and scoped
   normal environment/cwd. Record termios on the real PTY before the action.
   Read `claude agents --json --all` through the qualified supported native route;
   retain only the safe IDs/state needed to compare this invocation.
2. Invoke one background New through the declared compiled public or TUI entry.
   Observe actual job creation and **attachment to that job** on the same terminal.
   A process exit, registration alone or a success sentence is insufficient.
3. Correlate the actual job identity returned by its owner with exactly one new
   job in `claude agents --json --all`. Preserve all pre-existing jobs. The list
   difference alone is insufficient if it cannot be tied to this invocation.
4. Detach using the installed client's qualified supported control. Check that
   the attached terminal is released, termios equals its pre-action state and the
   same job remains running in a fresh `claude agents --json --all` readback.
   Attachment and post-detach liveness are separate observations.
5. Settle only the newly owned job using the separately qualified cleanup route.
   Keep actual native registration/attachment/detachment/cleanup receipts and
   source/build/PTY identities. Do not kill by process name or touch older jobs.

| Case | Required result | Wrong outcome |
| --- | --- | --- |
| F-BG-ATTACH | One new native job; terminal attaches to that exact job | Registration then return-to-picker prose with no attachment |
| F-BG-DETACH | Detach releases terminal, restores termios and leaves that job running | Detach kills/replaces the job, or leaves terminal modes changed |
| F-BG-COUNT | Exactly one newly owned job in `claude agents --json --all` | Duplicate jobs, invented identity or an unrelated existing job |
| F-STOPPED-OPEN | Stopped native Claude Details shows Open and no Resume in background; activate Open and observe the same native conversation UUID, saved settings and actual attachment | Removed action appears, Open creates a different session, changes saved settings or returns without attachment |
| R15-PUBLIC-NEW | Real compiled public parsing/dispatch/native process boundary preserves the adopted mode/argv/cwd/environment | A helper-only argv test or fake registration standing in for public wiring |
| R15-ARGV-VALUES | Original supplied argument tail preserved, including aliases, option values, delimiters and positional text | Router deduplicates, strips or scans native options |
| R15-RESUME-FORK | Supported native route retains actual session identity, saved settings and original args | A new job/session substituted for the captured one, or ACP identity reinterpreted as native identity |
| R15-NATIVE-CONFLICT | Actual qualified native client rejects incompatible modes before its native dispatch | A stand-in's rejection claimed as vendor behavior |
| R15-FAILURE | Spawn/native failure propagates with no fabricated success/ID | Silent success, automatic retry or a made-up background identity |

Controlled process tests may replace the native executable to establish argv,
cwd/environment and failure propagation. They cannot prove vendor grammar,
registration, actual attachment, detachment, liveness or real job settlement.
Historical source and process receipts retain only the claims they actually
proved; none establishes the corrected attachment behavior.

For F-STOPPED-OPEN, capture native identity/settings before the action from the
qualified existing read owner and correlate them after actual resume. Keep any
new job handle distinct from that native conversation UUID. A B returned-action
receipt proves only the captured effect target; it cannot establish vendor
settings preservation or attachment. The offline checks in
`scripts/verification/tui/journeys/scenes/claude-background-checks.ts` qualify only the
observation oracle, never the vendor interaction.

## Runtime prerequisites and retained evidence

The execution packet supplies exact compiled/native paths and hashes, supported
entry/mode/attach/detach/cleanup controls, approved environment/cwd, actual provider/
model/funding boundary, before/after job inventory and termios observation routes,
and owned cleanup authority. No native launch is authorized merely by reading it.
Requalify installed client behavior after any binary change.

Keep source/build/client bindings, safe command/result attribution, actual job ID,
attachment witness, detach witness, correlated native inventory, termios equality
and cleanup settlement. Do not retain routing tokens or personal transcript text.
Report each row observed, unsupported or unverified; later UI migration must
re-establish the real entry path at its own declared head.
