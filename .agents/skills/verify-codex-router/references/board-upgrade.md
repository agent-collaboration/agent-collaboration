# Board upgrade

## Current entry

The wrapper retired in 0.1.102. The retained Rust preparation, publication, replay and restart route is `crates/board-legacy-upgrade/tests/upgrade_binary_e2e.rs`. Prepare the real local entry points as CI does, then run:

```sh
cargo test --locked -p board-legacy-upgrade --test upgrade_binary_e2e -- --test-threads=1
```

The retained independent inbox oracle is `crates/board-legacy-upgrade/examples/legacy_selection_oracle.rs`; build/run that example with an explicitly supplied sealed copy, copy root and private output. Its owning tests remain in the example and upgrade suite.

A separately commissioned sealed-copy Mac observation uses `node verification/import-run.ts "$PRIVATE_DECLARATION"`; its current contract is `verification/import-declaration.ts`. It supports the declared local Tokio target and real copies on that Mac only.

## Retained obligations and historical evidence

The sections below preserve the governing obligations and earlier qualification limits. Descriptions of retired wrappers and their inputs are historical; they are not current command contracts. Use the retained source above to determine actual coverage, and report any obligation without a current route as unavailable. This documentation change supplies no new runtime proof or operating authority.

For a separately commissioned G4 external upload, use the
[external central route](board-upgrade-external.md). It adds the declared
HTTPS/owner Access and machine-credential inputs and qualifies locally before
any staging run; the local route below remains fixture-authenticated evidence.

This lasting journey drives the one-time upgrade through the actual
`codex-router-board-upgrade` CLI and an isolated real Tokio central. The local
route covers dry-run, publication, interruption on either side of publication,
owning reopen and rerun. Retained observations use synthetic legacy fixtures;
qualify the route on the declared target before crediting a new G3 run. Supplied
real copies use the same plan and driver when explicitly commissioned.

Use the board-upgrade launch below. The native central helper owns its Tokio
server and fixture JWKS server, issues a fixture Access assertion into a private
file, exports readbacks from its owning process, and drains both servers on
stop or stdin EOF. This proves fixture-authenticated local behavior; real owner
sign-in and staging remain separate observations.

The oracle is the board Verification Design OB5 and §5e, spec 1 §5 and spec 3
FL-15–FL-17 at design commit `bb149702`, with the Lead's PD2 §3d amendment at
`361b49f7`, metadata amendment `62976f16`, interval/root amendment `9c41521c`,
description correction `b16dbd83` (superseding `5e38f7f5`), and watch-coverage
correction `adc51bce` (PD1 inbox/subscriptions and PD2 §3d). The Lead also
requires the logical digest to exclude import-run provenance and owner
attribution. X2's report and artifacts are under
`tmp/design-workflows/2026-10-17-board-and-central-on-fleet-sync/` in the supplied
design checkout. They derive the rule; their scratch schema is not the product
schema. X2's unchanged oracle does not close the mixed main/reply bookmark case.

## Build and qualify the local route

Start from the caller's declared checkout/HEAD. The CLI uses normal `target/debug`;
the standalone native helper uses its own target below when commissioned. Check
disk before each package-scoped build; the current workstream requires at least
55 GiB to start and stopping below 45 GiB. Coordinate target ownership with any
paired observer. Use the existing profile settings and keep evidence outside
`target/`:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
  cargo build -p board-legacy-upgrade --bin codex-router-board-upgrade --locked --offline
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
  cargo build --manifest-path scripts/verification/board_upgrade_native/Cargo.toml \
  --bins --target-dir scripts/verification/board_upgrade_native/target --locked --offline
```

The second package pins agent-fleet-sync to `e221ffb51480a6e73476b610048e823f278a680d`,
matching the product's Registry/Delivery types. It builds `board-upgrade-native-observer`,
`board-upgrade-central` and the setup-only `board-request-shapes` decoder.
The central helper's `--help` describes its private JSON-line
control protocol. The protocol accepts only an explicit private system-temp
root and nonempty project IDs. Each fresh launcher produces a ready receipt
with its actual PID/loopback URL. The journey then checks the real owner
project-status route through the fixture JWT before driving the CLI. A port
that merely answers is insufficient readiness.

Run the importer through `scripts/cargo_debug_signing_runner.sh ... --help`
before pinning its hash, with any required signing authority already granted.
Record `codesign -dv` output separately. If signing would prompt, stop and report
the prerequisite; do not click it. A local behavior result does not establish
the release artifact's signing or Homebrew installation.

## Publication plan and drive

The caller provides sealed copies; never obtain real home stores yourself.
`copy_root` is an existing private, owner-controlled system-temp directory.
Every source is a canonical file below it, outside normal Codex/Router state,
with no WAL/SHM/journal sidecars and at least one project to exercise publication.
Seal/copy ownership is the supplier's responsibility. The driver reads only
the explicitly listed copies, checks their byte/mtime fingerprints, and never
writes them. The current fixture route can be prepared by copying the retained
test fixture into that caller-owned directory.

`PublicationPlan` in `board_upgrade_publication_views` (retired wrapper) defines the complete
JSON input. Set the caller's `DECLARED_SOURCE_HEAD`, `LEGACY_COPY_ROOT`,
`LEGACY_COPY`, `JOURNEY_PLAN` and fresh `JOURNEY_OUTPUT` first. The output's
parent must also be private system-temp state outside the checkout.

The retired command is unavailable; use the current entry and declaration above.

For a commissioned real-copy run, set `source_evidence` to `ownerSuppliedCopy`
and list the actual handed-over paths. That field records the caller's evidence
classification; it grants no access. Larger supplied inputs may raise the
plan's bounded `timeout_seconds` from its 180-second default.

`--scenario` selects `dry-run`, `publication`, `before-publication`,
`after-publication`, or the default `all` local phases:

- Dry-run checks report/log-only effects, unchanged source/identity bytes and
  independent legacy topic IDs, exact title/description, post counts and
  unknown imported Fleet time.
- Publication uses the actual CLI and owner routes. It checks real Following
  copies, seed publication receipts, manifests, Unheld/epoch-one authority,
  workflow completion and unknown-time Copy summaries/Fleet projection. It
  stops and reopens the owning central before a relabelled replay, then compares
  exact durable Copy/seed/summary/workflow/projection values.
- Before-publication interruption holds the first actual Complete request
  after pages are staged. The driver terminates and reaps only its CLI, releases
  its HTTP gate, then stops central and checks Reserved/no live posts/no
  summary. A new central process and CLI must retain the first header/import ID
  and finish the same attempt.
- After-publication interruption forwards Complete and obtains its actual
  Imported answer, then withholds that reply while the driver terminates its
  CLI. After owning stop/reopen, a changed owner/machine must answer
  AlreadyImported for that project and preserve its original physical state.
  The later projects finish normally; a further replay changes no durable state.

The wire gate is an interruption mechanism, not a replacement for central or
the receiver. The actual CLI, Access verifier, ImportTask, snapshot stream and
shared Copy seed execute. Interior publication/phase-receipt failure windows
also have the retained real-store cases in `upload_publication.rs`; keep those
distinct from this journey's actual process/wire interruptions.
Both `importConflict` forms are covered separately by that file's actual
uploader/ordinary-CopyCreate product case. This driver establishes same-input
replay; it does not count that product case as an extra journey assertion.

For sensitivity controls, pin an explicitly labelled wrong executable and its
source diff separately, restore product source before driving it, and use a
fresh evidence directory for each selected scenario. The deciding failures
are a wrong dry-run result, completion without durable publication, a pending
attempt refused after reopen, and a completed attempt answered Imported again.
A control that cannot build or reach its checkpoint is a harness gap, not the
required behavioral failure. No physical double-publication mutant is claimed.

## Source-entry inspection only

Use the current Rust/TypeScript entry above from the repository root. Supply the
declared source commit and a new output directory under an existing, private,
owner-controlled system-temp task directory:

The retired command is unavailable; use the current entry and declaration above.

The optional entry gate checks HEAD, clean product source (allowing only the assigned verifier
paths), and `cargo metadata --offline --locked --no-deps --format-version 1`.
It builds and launches nothing, opens no store and sends no model input.
`entry.json` retains commands/exits, source identity, driver hashes and unknown
coverage. **Exit 2 means entry refused, zero product assertions and zero cases
run.** Presence of a target never turns this gate into a pass. Build identity is
explicitly `notBuilt`; metadata is not an installed-binary check.

## Independent legacy selection

Use only fixtures or consolidated copies handed over by the Lead. Never obtain
real stores yourself. Copies and source-derived IDs stay private and out of Git.
The source must have no WAL/SHM/journal sidecars. The reader uses SQLite
`mode=ro&immutable=1`, checks its byte digest around the read, and rejects paths
under normal Codex/Router state. Sealing and confirming no live owner is the
copy supplier's responsibility; this oracle is not the importer's open-store
refusal proof.

The retired command is unavailable; use the current entry and declaration above.

`legacy_selection_oracle` (retired wrapper) projects the shipped inbox predicate at
`ffced1c1:crates/message-board-storage/src/inbox_records.rs:124` into each topic's
post selection. Main posts use the workspace main-start and topic bookmark;
replies use the active thread watch and thread bookmark. Unread omits the
reader's own activity. Latest ignores cutoffs and includes own posts, still
requiring active watches for replies. Topic bookmarks never read replies.
Reader/topic coverage comes from `board_identities` and `board_topics`, including
readers without marks or watches and empty topics. It is never inferred from
the state or activity whose absence the observation must check. A topic-only
watch does not subscribe to any thread's replies; an explicit watch on one
thread does not include replies from its unwatched siblings.
Receipts identify source/driver hashes and explicitly say no product import was
observed. `legacy_selection_oracle_test` (retired wrapper) checks literal mixed-history cases,
including rejection of an incorrectly shared main/reply cutoff.

Run the oracle's apparatus checks independently (these are not product tests):

The retired command is unavailable; use the current entry and declaration above.

## Offline native preparation fidelity

`--observe-plan` retains `ObservationPlan` in `board_upgrade_views` (retired wrapper): declared
repository/reference repository, expected HEAD, CLI/native-observer paths and
hashes, and a fresh private output directory. It drives the retained synthetic
fixtures and the independent native oracle. Its scope is preparation, not
publication. The driver's case list is the wider coverage checklist: preserve IDs,
all message/non-message history and attribution, dense epoch-1 ordering,
bookmark/watch selected post sets, membership policy/expiry/generation/endings,
unknown record times and deterministic digests. A draining membership ends at
the last source-recorded event time even if its thread reopened. Refuse
collisions without renaming; retain both conflicting IDs. Verify report-only
dry-run, unchanged source bytes and refusal while a Router owns the copy.

PD2 §3d's amendment gives the actual history homes: seat events and resolutions
become `Thread` record versions and `thread_participation` interval changes.
Seat actor/role/replaced actor come from each event's history columns, never the
overwritten current participant row; retain X2's reply 201/203/204/205 cases.
`posted_from_activity` becomes `posts.posted_as_role`. A new thread writes both
Thread and Post rows; every translated cutoff names the last feed row for its
source activity. Imported resolutions create no ThreadResolved lifecycle fact,
because the legacy store recorded no resolution reason.

The later amendments require these additional observations:

- The first row is `ProjectState` at epoch 1/position 1, including for an empty
  topic. Its actor is the declared Human owner, its origin is the supplied
  import provenance, its time is unknown, and its request is absent. It creates
  no ProjectCreated lifecycle fact.
- Description text round-trips exactly through `ProjectState.description`,
  including empty and maximum-size values with control characters. The fleet
  directory carries the name as its title and has no description. Description
  participates in the logical import digest.
- Each explicit join opens an interval and closes a previous open interval as
  `rejoined`. A handover to an already-open participant closes the target's old
  interval as `promoted` and opens its orchestrator interval; the outgoing
  holder closes as `replaced`. Closed intervals retain their role, note and
  original join key.
- Creator join and root post import as one creation, with either source cutoff
  mapping to the post's position. Root attribution comes only from the root's
  own `posted_from_activity`; an absent reference stays NULL even when the
  creator took a seat. No role is inferred from a covering interval.
- Logical project digest excludes import-run provenance and owner attribution.
  Repeating under a different machine label must answer `alreadyImported`;
  changing legacy content, including description, must answer `importConflict`.
- Preserve the exact legacy thread-watch row set: identities, active/inactive
  choices and translated starts. A legacy topic watch becomes a project watch;
  a subscription becomes its membership. Synthesize no missing thread-watch or
  `Unwatched` opt-out row. Absence and explicit opt-out affect later live
  subscription coverage differently.

On a tree containing A's watch correction, both native regression scenarios in
`crates/board-legacy-upgrade/tests/native_inbox_fidelity.rs` must pass unchanged:
the topic-only watch fixture gives the owner's unread reply set `{}`, and the
mixed topic/explicit-thread fixture gives `{reply 201}`, excluding reply 202 in
the unwatched sibling. Compare against the independent legacy oracle, then
inspect the imported watch row set separately; equal post sets alone would
miss a synthesized inactive row.

## Paired live-side observation

The live watch correction originates at `adc51bce`. Drive these cases through
the board-records journey on the declared integrated target. Imported-row
fidelity alone does not establish live watch activation.

Use a plain project watcher and a project subscriber as distinct readers of an
already-existing thread. A plain watch creates no thread watch: after a new
reply, that reader's inbox and unread counts contain no reply. Activating the
project subscription writes or reactivates watches for existing threads, so the
subscriber sees the later reply. Respect explicit thread-subscription overrides.
Inspect the real owning store's watch rows as well as inbox and count answers.
For new posts, only active, unexpired subscribers are pulled into threads;
plain watchers are not. A project watch is never a fallback for a missing thread
row. Ending a subscription or project watch leaves the thread rows it wrote.

Keep this live-side observation distinct from D's imported-row fidelity and
native inbox regressions. A passing import does not establish live activation,
and an A-only observation does not establish the imported stores.

## Message cutover and delivery policy

Decisions 1049–1052 require current board identities in stored activity ranges
and canonical expiry origins, with all other released receipt bytes copied
exactly. Copying must precede the actual legacy-table drop (903). Decisions
1057/1062 allow only the first unguarded Auto attempt to wake a known unloaded
session; an SDK Held item released back to a Pending projection is a later
attempt and stays LoadedOnly.

Use the permanent tests through their actual owners, from the repository root:

```sh
cargo nextest run -p board-legacy-upgrade --locked --offline --test-threads 2 \
  --test activity_range_cutover --test expiry_origin_cutover \
  --test message_cutover --test message_cutover_crash --test message_cutover_cli
cargo nextest run -p codex-router-host --lib --locked --offline --test-threads 2 \
  -E 'test(importer_claim)'
cargo nextest run -p collaboration-service --lib --test message_delivery_paths \
  --locked --offline --test-threads 2 \
  -E 'binary(message_delivery_paths) | test(mail_push_preparation::tests::) | test(mail_push_delivery::tests::direct::) | test(readiness_release_then_unload_does_not_load_or_submit_a_held_auto_again)'
```

The activity case must read mapped project/thread cursors through InteractionStore
and verify their covered imported feed; the expiry cases must read current Thread
and Project scopes, including an empty topic. Unmapped identities refuse without
destination creation or source mutation. The crash parent invokes its ignored
child, observes exit 93 after copy commit, and requires identical IDs/ranges/origins
after retry and the real drop. Do not count that helper skip as unexecuted parent proof.
The released-receipt case compares raw `last_outcome_json` before/after copy,
owning read, retry and drop; no receipt normalization is allowed.

The two `importer_claim` cases use actual publication, Host Claim, epoch-two
append and central acknowledgement. Interrupted import completion must preserve
the advanced copy's holder, fence and prefix. These cover claim/post-claim locally;
they are not cross-machine G3 or hosted proof.

For Auto, retain both fresh public wake and readiness release/unload. The latter
uses the committed real release transaction as its barrier, asserts Pending
projection plus SDK Held identity, then changes Running to Wakeable. It must stay
Held with one native actor call total. Public fresh Auto separately requires the
resume/start and stored-notice correlation. Preserve Queue, Steer, guarded and
physical cold-reopen nonloading checks; fixture peers are not live vendor clients.
When commissioned, wrong builds that raw-copy either translation, recompute
from Attempted, or ignore the SDK later-attempt identity must fail the respective
unchanged checks. Keep mutants outside the product tree.

Before a G3 drive, qualify request construction without launching a Host:

The retired command is unavailable; use the current entry and declaration above.

This decodes actual shared-builder outputs with the current Rust DTOs, then
requires retired `observedEpoch`/`resubmits` additions to fail. A source sweep also
rejects those inline fields and the removed `--observed-epoch` flag throughout
board journey scripts. The check proves wire setup, not semantic admission or G3.

## Publication and cleanup

The publication helper exports Copy/seed/workflow/projection readbacks inside
the process that owns central, and exports the final readback after its full
shutdown. It never opens a live store from an unrelated reader. Completion
requires publication. Claim/post-claim now use the real Host cases above.
`import_http_failures` covers local typed 410/retired, 503/copyUnavailable and
503/centralCopyFull, uploader decoding and same-ID recovery; this controlled
storage fault is not physical storage exhaustion. Router-cache/cross-machine
composition, real copies, actual owner sign-in and G4 staging remain separately
commissioned observations. Building the helper or checking its wire DTOs does
not run any of those journeys.

Preserve exact binary hashes, signed-runner commands, exits, assertions and
private readbacks outside `target/`. Coordinate builds and cleanup with the
Implementer. Stop no preexisting process. The entry/oracle commands start no
process that survives their exit and delete no evidence. The publication drive
retains `journey.json`, CLI logs/reports, exact wire checkpoints and owning
readbacks. Its cleanup reaps its exact child handles and closes its HTTP gate;
it records and removes only its terminated CLI's disposable writer files, with
all evidence outside that directory. Reopen `journey.json` after teardown.

Apparatus checks (these are not product-test counts):

The retired command is unavailable; use the current entry and declaration above.

Return the declared source and actual executable hashes, fixture/supplied-copy
classification, commands/exits and per-phase assertions, retained evidence and
cleanup status. Keep entry checks (zero cases), apparatus tests, journey
assertions and product-test cases as separate evidence layers. Acceptance
remains with the Lead.
