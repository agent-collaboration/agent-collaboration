# Native fleet observation scripts

These four scripts drive real native fleet owners through authenticated sockets,
then inspect the responses and durable state. Their oracle is the retained
agent-fleet-sync `b9470dc` consumer-fit SO52/SO53/SO56/SO57 journeys, with the
caller's adopted changes and today's protocol contracts. The implementation is
`crates/fleet-native-services/examples/fleet_consumer_fit/main.rs`; its private
consumer/process fixtures remain in the same package. This is local native
evidence, separate from workerd adapters and hosted sessions.

The caller supplies a declared immutable product source, any apparatus diff,
exclusive build/observation slot, permitted normal target and fresh owned
evidence/state directory. Return each journey's actual source/artifact hashes,
JSONL observations, exit, gaps, durable readbacks and owned settlement. A recipe
or build without that observation leaves its selected journey unverified.

## Select and launch

Run the matching entry from the repository root; `--help` exits before building
or creating evidence. Each supplied evidence directory must not exist, and its
parent must be within the declared write scope.

| Entry under `scripts/verification/` | Required real-path outcome                                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `holder-refusals.sh`                | AlreadyHeld names the original holder in both stored and returned refusal, then replays after takeover. A real writer's migration 7 rolls back; its actual Pending release answer crosses the socket to the registry and replays after the pending offer is removed. No real writer produces Failed; separate registry pass-through is not that producer.                                                                              |
| `target-capability.sh`              | Per-target support and frozen refusal replay; independent supported grant; move support/no-report controls; admission frozen before drive and across restart; report/Welcome/finalization/close ordering. The production pump refuses dispatch before Welcome and serves after it; an unstarted production pump's real local reply deadline yields AnswerLost. Scripted holder release proves registry fencing, not writer draining.   |
| `frame-shapes.sh`                   | Five identified bad bodies get exact correlated InvalidShape fields and five valid same-socket follow-ups, including mixed-width Unicode capped at 64 characters. Four unreadable envelopes close separate connections as MalformedFrame, with no routed operations.                                                                                                                                                                   |
| `copy-containment.sh`               | Damaged and healthy real writers share one observed connection; typed damage and unknown outcomes preserve sender state while the sibling progresses. Exact repair and identical Piece replay reconcile once, including dropped committed answers, commit/rollback uncertainty, registry claim/move restart, owed wake recovery and pinned seed begin/page/complete. Closed-codec controls remain distinct from transaction certainty. |

```sh
df -h .
git rev-parse HEAD
git status --short
env -u GITHUB_ACTIONS CARGO_BUILD_JOBS=2 bash scripts/verification/holder-refusals.sh --evidence-dir "$EVIDENCE/holder"
```

Substitute the selected script and a fresh directory for another journey. The
launcher builds `fleet-native-services` fixture binaries and its
`fleet-consumer-fit` example with `test-support,test-fault` in the assigned
normal `target/debug`; below 30 GiB available it refuses the build. It rejects
a different `CARGO_TARGET_DIR`. No standalone workspace or extra lock is used.

`--no-build` is for already qualified artifacts of that declared source/config;
retain the prior build and exact executable hashes. It still performs signing
and signature/team checks. Do not infer provenance from a filename alone.

## Signing, readiness and observations

Before any direct child launch, the shell invokes the project's signing runner
with `--sign-only` on the exact consumer, delivery fixture and example. On
Darwin it requires strict signatures and `TeamIdentifier=974QD84WVC` on all
three, then launches the example through the runner. A signing refusal is a
setup failure. Keep the existing signing policy and caller's actual host grant.

The example receives the explicitly declared normal-target fixture paths;
missing executable variables fail instead of guessing a location. The process
fixture requires its child PID in `READY`, followed by authenticated matching
Welcome. A listening socket or READY alone does not establish session readiness.
The observing transport records actual frames and holds reconnect where one
connection is the assertion. Reconnecting cannot satisfy containment.

Read `observations.jsonl` together with retained fixture databases and the
per-case expected/observed values. Physical marker damage and exact restoration
are confined to owned fixture copies; they do not authorize account/production
storage changes. A lost real committed response, unknown transaction result
and failed post-commit wake are different boundaries. Pure codec passes do not
establish a producer's rollback certainty.

Exit 0 means the implemented journey produced nonempty observations with no
mismatch or gap; 1 means mismatch; 2 means harness/setup failure; 3 means
coverage incomplete. Zero observations cannot pass. Read the raw summary even
on a nonzero exit, preserving earlier successful observations without calling
the whole route complete. JSONL observation counts are not Rust test counts.

Copy containment deliberately emits `registry-copy-call-detail` and
`workerd-copy-certainty` as separate-proof gaps and exits 3 even when all native
controls pass. The first requires separately qualified real-engine NoEffect,
reopen and closed-view controls; the second requires the real workerd
adapter/classifier and engine receipts. IF adapter/migration workerd proof is
commissioned, load [fleet workerd](fleet-workerd.md) and return its scoped case
receipts separately. Do not rewrite the native summary after another suite.
Its named engine gap stays open until its actual target is supplied and driven.

## Evidence and settlement

Keep source HEAD/status/fingerprints before and after, build/signature logs,
executable hashes, raw frames/observations, exit code and fixture stores. A
source change during observation emits a gap and invalidates reuse without
qualification. State and evidence belong outside Cargo outputs. Each fixture
service, pump, store and child stop/kill is owned and awaited; confirm their
settlement before release of the build slot. Preserve failure evidence before
cleaning only this task's residue under the repository cleanup policy.

The example's deterministic completion controls are included in ordinary
nextest discovery through its `test = true` target and required fixture features.
Run `cargo nextest run --locked -p fleet-native-services --features test-support,test-fault -E 'binary_id(=fleet-native-services::example/fleet-consumer-fit)' --no-tests fail`
when the observation summary or missing-case handling changes. These four
apparatus tests do not replace any native journey or its durable readbacks.

Complete this selected observation only with its actual results and named gaps;
no route authorizes production Router replacement, provider input, deployment,
release, upstream deletion or merge acceptance.
