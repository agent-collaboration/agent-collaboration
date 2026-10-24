# Real board journey

`board_real_app_journey.py` drives the built `agent-collaboration` CLI over its
private Unix MCP listener and the same Host-owned `board-debug-runtime` over TCP
MCP. It checks observable identities, content, state and failure outcomes. It
does not build binaries or contact an installed Router.

The driver uses stdlib-only Python 3.12+, an owned private `/tmp` directory per
case, and a checkout with clean tracked product source or a verified Git export.
Only this driver's own `scripts/verification` edits or untracked files are
permitted beside the declared product head; their status and hashes are recorded
separately. It records the
source commit, source-export check, binary SHA-256 digests, driver digests,
service ID/epoch/listeners, exact calls and responses, expected/observed outcomes
and owned shutdown facts. Assertions and successful calls are counted separately.
Existing output directories are refused. Temporary runtime roots are retained
for inspection; the script removes no directories and signals only children it
created.

Run after building the launcher and CLI from the declared source:

```sh
python3 -m scripts.verification.board_real_app_journey --list-cases
python3 -m scripts.verification.qualify_peer_fixture \
  --output tmp/verify-m2/peer-producer-qualification.json
python3 -m scripts.verification.board_real_app_journey \
  --expected-head <full-source-commit> \
  --peer-qualification tmp/verify-m2/peer-producer-qualification.json \
  --output-directory tmp/verify-m2/observations/final
```

For the round-2 sensitivity run, point `--source-root` at the separately built
`git archive 61b46682` export and select `--case rejected-scope`,
`--case project-watch` and `--case membership-restart`. The actual baseline
failures must be inspected: a setup/import/build failure is not behavioral red.
Do not share the Implementer's target while it is building. The final delivery
report owns the exact build commands, identities, exits and raw evidence paths.

The cases observe:

- G1–G3: actual hook-context resolution, all three aggregate search scopes in
  default/archive-inclusive modes around a rejected creation, and initialized
  workspace unread transitions. The rejected-scope group also stops the Host,
  replaces only the rejected B orphan file with 8 KiB of `0x58`, and restarts
  with a fresh registry. Exact search/context/summary results remain, aggregate
  post/inbox pages exclude B from `unavailable`, and direct rejected-request
  replay still refuses the damaged ledger. The fault receipt names the owned
  file, before/after digests, exact bytes and unchanged sibling files; its
  read-only SQLite probe is fixture qualification, not native product proof.
- G4: governing plan, role, exact own pending work, work dependencies/checklist,
  owner blocks, lead obligations, actual read marks and foreign pending corruption.
- G5: immediate human poll subscription, exact ranges, no repeats, policy
  generation/activation and owed-range/content recovery after restart. The public
  subscription list must include its required `unavailable` field: empty for a
  healthy reader and the exact contained B project beside A's retained rows.
- G6: schema/watch isolation, same-reader membership corruption at restart,
  saved-position aggregate recovery, and duplicate semantic ACK refusal without
  consuming entry IDs or moving coverage.
- G7: keyed accepted/rejected replay, conflicting content, side-effect-free
  checks, rejected/accepted moves, linked settlement, redaction and causal late facts.
- M2b role authority: reviewer/advisor/operator grants, exact RoleRequirement and
  pairNotDistinct refusals, reverse distinctness, Many holder retention/release,
  repeat-grant AlreadySo and the lead-grant exception. Keyed role operations run
  through the real CLI and replay over TCP MCP with exact request-result
  equality. Complete stored assignments are also read through both listeners
  and compared exactly. Mutable reader state is freshly recomputed on replay;
  both snapshots remain in the raw receipt, rather than being treated as frozen.
  CLI comparisons unwrap the documented `result.record` envelope; the complete
  original envelopes stay in the raw receipt.
  A lead also holding advisor, reviewer and operator roles writes with no
  explicit work-trail subject; readback must contain its implicit Project subject.
- M2b thread subjects: explicit project-wide creation as a role holder,
  non-holder refusal, omitted-about refusal, and a re-file with the exact
  ThreadRefiled from/to record, replay/no-op and unauthorized re-file checks.
  Anonymous thread/subject read results match exactly over CLI and MCP, and the
  displayed title follows a subsequent workstream rename. The same non-holder
  refused a project-wide subject can open a referenced-workstream subject.
  All five subject forms cross the actual CLI parser, with keyed MCP replay
  and exact anonymous thread readback. Task, row and ADR subjects name real
  records created through admission; their literal expected titles include the
  row's first line. Each non-project root uses a distinct non-holder author.
  Existing journey roots supply explicit subjects and use actual role holders.
  The multi-root pagination fixture grants distinct advisor authors so its roots
  retain the ordinary cooldown and authority checks without sleeps. Both CLI
  and MCP refuse omitted subjects and the removed project-placement post entry.
  Membership recovery likewise keeps a distinct advisor root author, so its
  subscribed reader has actual owed activity rather than its own post.

- M2b role notifications: two local declared session recipients, exact post-cause
  intent identities and separate BoardNotification push records, read through
  MCP and the real CLI as each recipient. A read-only owned automation-store
  locator supplies unknown push IDs; public API reads prove the payloads. A
  bounded macOS kqueue SQLite write-event wait observes actual storage, without sleeping or
  simulating a receiver. The Lead's M2b ruling defines this as the app boundary;
  native session arrival is not claimed.

Forced pre-handoff re-resolution is native claim/store integration proof under
the Lead's ruling: A-to-B-to-A, frozen handoff, named actor and empty-set
transitions need actual claims, not static views. The app does not add a pause
seam. TH-4 upgrade/import execution remains M5.

G1 uses a named session-entry-author stand-in, qualified against the pinned
shipped Claude Code 2.1.301 writer and serializer, rather than against only
Router's reader. A changed producer executable fails qualification. Native
session creation, its own publication and turns remain unverified until M4.
`board_journey_peer.py` records the producer pin and field construction.
The qualification command runs the actual extracted producer object expression,
status merger and serializer in Node with declared controlled inputs. It compares
exact bytes and rejects the old missing-field/newline forms. This maintenance
check needs the installed pinned executable and Node; it starts no native
session. The runtime driver stays Python stdlib-only and binds that receipt to
the exact fixture-module hash and the bytes it publishes.

O-R2-F2's transient scan failure has no real-app injection/observation seam.
The Lead assigns its proof to the real-store integration test plus a temporary
quarantine mutation in an isolated export. This driver does not manufacture a
runtime claim for it. Detailed 8/80 interval and >100 tagged-row cost cases and
the unchanged maximum-identity release budget likewise belong to their named
integration gates.

After the Implementer's final-head/build handoff, the commissioned transient
check is runnable with `python3 -m scripts.verification.verify_transient_scan_sensitivity`,
passing `--source-export`, `--expected-head` and a new `--output-directory`.
It verifies the exported source, mutates only the exact existing error-class
arm, requires the named quarantine assertion to fail, restores the source in a
`finally` block and requires the exact test to pass. It refuses the product
worktree and shared targets. The caller owns build scheduling, disk checks and
the export target's eventual Cargo cleanup.

This journey does not prove native Codex/app-server lifecycle, a native Claude
turn, real central admission/leases/replication, remote-watch propagation or P3
external delivery. The local Fleet/lease/C5 composition is named as a stand-in;
the release and cross-machine outcomes remain M4 and later.

`board_list_subject_journey.py` observes the M2b review correction S-M2b-R1-F1
on the real Host. It is runnable with
`python3 -m scripts.verification.board_list_subject_journey`, passing
`--expected-head`, `--manifest` (the frozen CLI-last artifact manifest) and a
new `--output-directory`. It compares the repository-path, no-selector and plain
default thread lists with the workspace CLI and MCP rows through a rename, a
refile, a redacted work-trail row and an ADR missing from the fleet cache.

The missing ADR is an offline edit of the debug Fleet stand-in's snapshot and
cache inside the journey's own root. It proves display privacy when a subject
can't be resolved; it does not prove a production central-to-cache loss.
