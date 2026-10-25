# Board records, replication and holder changes

Use `scripts/verification/board_central_journey.py --plan PLAN.json` for the
model-free local central route. It drives prebuilt `board-debug-runtime` Hosts,
`agent-collaboration` over private Unix listeners and TCP MCP, and the real
`board-upgrade-central`. The launcher accepts a private fixture session file.
Registry enrollment, leases, Copy, snapshot installation, notices and board
policy remain real. The fixture Access edge adds only the repository's signed
local test assertion to the HTTP handshake and forwards WebSocket bytes unchanged.
It supplies no board decisions. No provider or normal Router is started.

Adopted outcomes: board Verification Design §2 and OB1–OB4; Program Design 2 §3a;
Program Design 3's Directory/copy/lease, archive and notice contracts. I1 uses
committed lease identity: derived Held/Expired display state is excluded from
equal-version identity; holder, epoch, expiry, variant and metadata asOf remain
committed facts. The governing design files live under
`docs/specs/2026-10-04-board-orchestration-design/` in the design tree. The caller
supplies that tree when this checkout contains release placeholders.

## Declared artifacts and launch

Provide qualified prebuilt `target/debug/agent-collaboration`,
`target/debug/examples/board-debug-runtime` and `target/debug/board-upgrade-central`.
The central helper source is `scripts/verification/board_upgrade_native/src/central.rs`.
Coordinate its build with the shared build owner; this driver never builds.
The caller's `artifacts` JSON array contains `path`, `sha256`, `identifier`, `team`
and `strictSignatureVerified` for each artifact. Retain actual build/signing
receipts. The driver checks bytes before and after use, and never infers source
provenance from a filename. The `joint` scenario also consumes the qualified
`target/debug/codex-router-board-upgrade`; other scenarios exclude that artifact.
A changed consumed hash stops the run. Build every consumed artifact at the
declared pin and finish Cargo before signing and hashing: later Cargo commands
may replace a signed executable with a linker-signed cached artifact.

Use a fresh private caller-authorized cache directory and private `r/` child.
Set TMPDIR to the cache root in the command environment; the launcher then uses
that owner-controlled temporary home. Keep runtime and evidence outside target/,
and paths short enough for macOS Unix sockets. Do not redirect HOME or inspect
provider transcripts. The model-free launcher requests no native backend.

The plan JSON has these caller-supplied bindings:

```json
{
  "repository": "/absolute/declared/worktree",
  "expected_head": "FULL_SOURCE_COMMIT",
  "artifacts": "/absolute/private/cache/shared-artifacts.json",
  "central_binary": "/absolute/declared/worktree/target/debug/board-upgrade-central",
  "runtime_parent": "/absolute/private/cache/r",
  "output_directory": "/absolute/private/cache/new-evidence-directory",
  "scenario": "records"
}
```

`expected_head` must be exactly 40 hexadecimal characters. Required `scenario`:
`fresh-create`, `records`, `replication`, `holder-change`, `archive`,
`verified-origin`, `joint`, or `i1`.
`record_cases` optionally selects `recorded-work`, `pending-views`, `role-authority`
and `thread-subjects`; the default runs all four. `known_unrelated_changes` is
empty unless the caller explicitly grants source/build equivalence for exact
unrelated paths. The receipt records intervening committed paths and rejects
changes outside that set and the verification apparatus. This grants no edits.

`source_root` optionally names an independently verified Git export while
`repository` remains the actual checkout that resolves the declared commit and
owns the normal artifacts. Every exported tracked blob is checked before launch;
this keeps live WIP separate from an observation of frozen product bytes.

```sh
TMPDIR="$BOARD_CACHE" PYTHONDONTWRITEBYTECODE=1 uv run --offline --no-project \
  --with pydantic==2.14.1 --python 3.13 python \
  -m scripts.verification.board_central_journey --plan "$BOARD_PLAN"
```

Readiness is the central ready line (owned PID/loopback URL) and each Host's
`boardDebugRuntimeReady` matched to v3 `service.json`. The driver boots a Host
without a session, reads `installation_show`, stops it, enrolls its actual
persisted peer through the owner route, and reopens with private fixture inputs.
`endpoints_list` checks the owning epoch before record cases. Initial connection
may answer `pendingUnreachable`; setup retries only the same explicit request
identity, bounded to 30 seconds, and never counts pending as a board mutation.

## Deciding observations

| Scenario | Deciding evidence |
| --- | --- |
| `fresh-create` | CLI exit 0 and successful MCP result, each containing the accepted project or `projectCreationPending` with its identity. A rejected decision cannot satisfy it. |
| `records` | Four record/view/role/subject cases, accepted/refused replay, exact settled CLI/MCP answers, independent §3a readback and owning stop/reopen. Representative OB1 coverage; not every V1/V2/V5/V8 obligation. |
| `replication` | A writes publicly; B pulls the real central copy at A's independently observed feed position. Exact post, work lanes, acknowledged prefix, enrolled origin and keyed replay; owning Host reopen. |
| `holder-change` | Actual owner Move and takeover preview/confirm, native successor at the next epoch, preserved acknowledged lanes and refused superseded writes. Empty-board archive and notice progress are separate checks in the run. |
| `archive` | Independently drives empty-board archive and verifies visibility on B plus advancement of B's durable machine-notice cursor. Full nonempty-board final reporting remains separate. |
| `verified-origin` | A nonholder sends a project-qualified post through its public Host. Require acceptance at the holder and the sending installation in the stored origin. Copy-origin equality alone cannot satisfy forwarding. |
| `joint` | Real CLI import of a private synthetic legacy copy creates two unheld projects. Public Claim A installs epoch 2, owner Move to B installs epoch 3 and owner Takeover back to A installs epoch 4. A fresh write must replicate at every epoch, acknowledged old lanes remain readable and a stale holder's task is refused. B's public MCP creates a root and reply, checks exact enrolled origin/declared actor/text and same-ID replay without another post. An unheld project returns `notHeld`; after closing A, B's CLI exits zero with `holderUnreachable` and the independently observed lastSeen. A owning reopen must still publish epoch 4. |
| `i1` | Pin only Fleet's conformance clock to grant expiry minus 1 ms and expiry. Directory shows Held then Expired at the same leaseVersion/asOf; B's readCopy view stays readable and identical. WorkView.currentWriter retains the committed Held epoch; Expired is Directory display state. |

Calls, deciding assertions and failures are retained in `journey.json`,
`journey.log` and runtime logs. Setup failures are never behavioral reds. Exit 0
means selected assertions passed; 1 records a failed drive; 2 records cleanup
failure. Inspect the cause before classifying any red.

The records route settles callers and closes the Host before SQLite mode=ro /
query_only readback. It refuses live consumers and verifies unchanged durable
database/WAL bytes. The independent §3a oracle checks all 31 typed tables,
UUIDv7 TEXT NOT NULL keys, business keys, absent retired tables, nonempty dense
feed and Router times. Logical reopen equality compares typed board tables;
CDC/capture acknowledgement/Turso maintenance stays in the raw readback but
is not an immutable board record. The logical notify-intent `(cause, recipient)`
key is stored as `(cause_kind, cause_id, recipient_key)`.

Shutdown requires Host exit 0, closed listeners and absent publication, then
central drain/exit 0. Recheck consumed hashes afterward. Retain evidence outside
build output. Remove exact owned residue only when no consumer remains.

## Qualification limits

This route does not claim exhaustive OB2 splitting/gaps, dropped connection or
central restart, schema/too-new-store cases, every holder refusal, archive
reservation races, fencing crashes, local/remote notification delivery or the
production lease timer. Permanent native cases below provide separate
source-qualified routes. Worker, hosted and two-machine coverage stay separate.
A missing producer or entry is named rather than synthesized.

For `joint`, the adopted forwarding oracle is spec 2 WA-13, spec 3 FL-14/15,
spec 1's typed outcome table and VD U56/OB4. Non-post writes are not forwarded;
the accepted lookup limit permits `scopeUnavailable/projectNotFound` on a Router
that never held that project, while a superseded old holder can return
`notWriter`. Forwarded `WriteReply` is a normal CLI result with exit 0, including
typed refusals. Post uniqueness uses the typed `{kind: post, id}` reference:
the root's Thread record intentionally shares its ID with its Post record.
The fixture Access edge has no selective answer-hold seam. The public
`unanswered` deadline remains a named gap; the permanent native
`forwarded_post_unknown` case holds the actual write gate across the production
deadline and checks same-ID retry with one post. That component case is not
two-Host public deadline evidence. No overload or missing-session coverage is
implied by the selected joint cases.

The I1 whole-struct wrong-build control requires an explicitly coordinated
fault build and receipt naming the deliberate mutation. It must fail the expired
nonholder read after a successful Held read. Compile/setup failures do not count.
Keep shared artifacts unchanged.

## Archive report components and the I1 control

These permanent native entries exercise the surviving report contracts. They
are component observations; neither establishes a public Host report producer.
Build only under the caller's granted window, with TMPDIR bound to its private
cache child:

```sh
cargo test --locked --offline -p collaboration-central-tokio --features conformance --test fleet_storage archive_settlement::final_report_and_archive_completion_roll_back_and_retry_together -- --exact --nocapture
cargo test --locked --offline -p collaboration-central-tokio --features conformance --test archive_reports archive_reports_preserve_first_report_and_exact_no_ledger_finalization -- --exact --nocapture
```

The first forces a final board-write failure through the actual Fleet object,
checks complete rollback, retries successfully and repeats without another
report or ledger effect. The second uses actual native sessions and a signed
Access fixture to check attribution, first report preservation, typed refusals
and exact board/workspace finalization. Reuse prebuilt executables only with
source/build applicability recorded; a retained path alone proves no build.

Rejected-tail report reads/answers are retired by the current protocol
(`board-sync-protocol/tests/retired_takeover_contracts.rs`). They are not
successful report-to-retry coverage. Keep a caller's broader reporting claim
open when no surviving entry or producer establishes it.

The permanent native I1 selector is
`collaboration_runtime::board_composition::tests::central_directory_expiry::native_directory_expiry_refresh_keeps_the_host_read_generation`.
For an explicitly granted wrong-source export only, change the equal-version
predicate from `!same_committed_lease(stored, next)` to `stored != next` in
`collaboration-service/src/central_project_reads.rs`. Build the Host lib test
with `--features runtime-test-support` in the caller's exact approved fault
target, leaving shared artifacts untouched. Execute that one selector with
`--exact --nocapture`: a valid Held read must precede an expired-read
`BoardUnavailable`. Save source/blob hashes and raw failure, restore every
exported blob, rebuild both service and Host fresh, and require the same case to
pass. Build/setup failures do not qualify. Remove only the exact granted fault
target after all consumers settle and text evidence is retained.

## Router holder-side claim journeys

This native path uses the actual central ImportTask/admin seed, a genuine
BoardProjectSchema snapshot, the normal Host session, native InstallCoordinator
and registry. It uses the real central ownership model described above.
Declare a source/window first; use normal target/ and retain evidence outside it.
It proves no Worker, hosted deployment or whole-PR completion.

```sh
cargo test --offline -p codex-router-host --lib claim_ -- --nocapture
cargo test --offline -p board-sync-protocol --test project_claim_contract
cargo test --offline -p collaboration-mcp --lib claim_schema_requires_attempt
cargo test --offline -p collaboration-mcp --lib success_schema_branches_match_main_golden_snapshot
cargo test --offline -p collaboration-mcp --lib catalog_tests
cargo build --offline -p agent-collaboration --bin agent-collaboration
scripts/cargo_debug_signing_runner.sh --sign-only target/debug/agent-collaboration
BOARD_CLAIM_CLI_BINARY="$PWD/target/debug/agent-collaboration" cargo test --offline -p codex-router-host --lib claim_cli_installs -- --ignored --nocapture
```

Set the CLI artifact path before starting any runner that snapshots its
environment. The test fails if it is absent. The actual command is
`agent-collaboration project claim PROJECT --request-id ATTEMPT
--service-directory API --json`. No actor supplies authority; enrollment does.
`board_composition/tests/project_claim.rs` checks the exact imported prefix and
schema, then the first guarded board write at the granted epoch.

The six real refusal sources live under
`crates/codex-router-host/src/collaboration_runtime/board_composition/tests/`:

| Refusal | Source and deciding test |
| --- | --- |
| NotFound | Missing central project; claim_not_found_and_archived_refusals_create_no_local_writer in claim_refusals.rs. |
| Archived | Actual archived canonical board plus imported project/Copy; same test, no seed. |
| AlreadyHeld | Real held project and another claim attempt; claim_already_held_preserves_the_native_peer_without_another_seed compares native PeerId. |
| TransferInProgress | Owner retirement held at Registry-to-Copy dispatch; claim_observes_real_retirement_in_progress_then_retired_without_seeding retains actual transfer identity. |
| Retired | The same retirement completed and retried; same test. |
| SchemaTooNew | Genuine future-schema snapshot imported into Copy; claim_schema_too_new_preserves_the_actual_copy_identity_and_supported_version in claim_future_schema.rs compares native identity/support. |

`claim_lost_grant.rs` arms TestCentral::drop_claim_grant_once for the actual
Claim attempt. The SignedAccessEdge selector in claim_grant_drop.rs correlates
typed Claim/frame identity and drops the first real typed Granted answer,
retaining its receipt and closing that socket. Pending cannot satisfy it.
The same-attempt retry must match captured epoch, expiry and start exactly.

`claim_renewal_order.rs` holds a real normal runner renewal, proves its drain,
then holds the bootstrap renewal while registry admission stays unavailable.
Publication follows persisted current authority. The normal engine is prompted;
a separately timed periodic-expiry experiment is not implied.

`claim_restart.rs` owns two real child processes: SIGKILL after synced rename
and before Ready; then both existing_project/open_project checks before any
replay Claim or preparation, independent native Seeding write refusal, and
resume. Compare actual public InstallRecord.request.attempt and install_id
across the crash. Echoing the input request ID is insufficient. Public record
reads use a configured, non-running NativeWriterRuntime with known IDs, not
private control-table queries.

Two independent controls must fail that parent:

- bypass only bootstrap reopen classification, so the two registry checks
  fail before any preparation repair (the original control retained the old
  repair to exclude masking; the accepted component removes that repair);
- mint a fresh SDK attempt on resume, so the actual Ready record's changed
  attempt/install_id/generation is rejected against the pre-crash record.

Retain exact source restoration and the positive rerun. Setup/compile failures
are not semantic reds. Acceptance and current-source qualification remain with
the commissioner. This index changes no verification driver or runtime grant.

## Receiving grants, Root records and notice acknowledgements

These use two real enrolled roots on the same declared central. Qualify the
source and normal artifacts before running; component-level results stay
separate from public CLI/MCP observations.

```sh
cargo test --offline -p codex-router-host --lib board_composition::tests::received_grants:: -- --nocapture
cargo test --offline -p codex-router-host --lib claim_and_received_notice_join_one_record -- --nocapture
cargo test --offline -p codex-router-host --lib receiving_restart_resumes_acknowledged_install -- --nocapture
cargo test --offline -p codex-router-host --lib received_grant_record_failure_blocks -- --nocapture
cargo test --offline -p codex-router-host --lib install_attempts::tests -- --nocapture
cargo test --offline -p codex-router-host --lib notice_receipts::tests -- --nocapture
```

List the selected tests before running. Exclude ignored child discovery from
results: restart parents launch those children themselves. `claim_` overlaps
some receiving-grant cases; do not count repeated selections as distinct coverage.

Sources under
`crates/codex-router-host/src/collaboration_runtime/board_composition/`:

| Journey | Deciding observation |
| --- | --- |
| `tests/received_grants.rs`: Move then Takeover | Two actual Router roots and enrollments share one central. A write at epoch 2 reaches Copy, Move installs epoch 3, and a replicated epoch 3 write survives Takeover into epoch 4. Old epoch 2 is unreadable during reacquisition, its bytes and Root choice stay unchanged, and a stale old-store signal cannot close the new writer. Duplicate, stale, future and other-holder hints create no new install; unknown Copy fails closed. |
| `tests/received_grants.rs`: higher epoch during held seed | Hold the original install after rename, transfer actual ownership, then require the old runner to stop before replacement proceeds. Compare the retained SDK request, install ID, generation and Root bytes. Replacement never publishes the old claimant as the new owner. |
| `tests/received_grants.rs`: same-epoch Paused owner | A public SDK renewal observation supplies a conservative zero local deadline. Preparation may decline or join, but must preserve the existing owner; real renewal restores writing and the next write replicates. This does not observe transport expiry. |
| `tests/claim_notice_overlap.rs` | Actual MCP Claim and its received notice share one generation-1 SDK record and inode. Its notice is acknowledged before Ready; another project's real creation and later Superseded notice still complete and acknowledge while the first seed stays held. |
| `tests/claim_restart.rs` and `tests/snapshot_install_child.rs` | Receiving-only resume crosses SIGKILL after acknowledgement and rename, before Ready. Cold existing/open/Seeding refusals precede pump startup or preparation. Compare the actual SDK attempt, install ID, generation and Root bytes; receiving resume invokes no Claim port. |
| `install_attempts.rs` and `tests/install_attempts.rs` | `<RouterRoot>/snapshot-install-attempts/<project>-e<epoch>.json` stores typed project, epoch and fresh UUIDv7 attempt. Reload and six contending creators keep one choice; another epoch gets a different choice. Invalid native identities, mismatched scope and unknown fields fail without replacing the recorded bytes. |
| `tests/notice_acknowledgement.rs` | Make only the receiver's own attempt directory unwritable and require a real permission failure. Decision1031 requires Failed for X and deferred/unacknowledged Y, with the transport cursor before X and no X file or Root choice. Restore permissions, reopen the actual owners, recover the inner X/Y prefix, then use a fresh real notice to establish outer ACK progress. A second reopen retains the same Root bytes and SDK generation-one Ready receipt. Inner machine-read positions and outer delivery positions are separate coordinates. |
| `notice_receipts.rs` | Literal received positions prove that out-of-order completion, failed handling and uncertain acknowledgement cannot advance beyond the handled prefix. These unit cases complement the real session/storage fault above. |

Retain controls that bypass authoritative hint confirmation, reuse an old
attempt/file, close by project rather than store identity, skip Copy bootstrap,
acknowledge before handling, or await Ready before acknowledging. Each control
must reach the relevant semantic assertion, restore the exact source, and pass
the original positive route afterward. Compile/setup failures establish none
of those outcomes.

The permanent idle-holder case in `tests/received_grants.rs` performs Claim
then Move without an intervening write and checks the inherited Copy prefix
and exact target record. Its independent control releases only the empty
current-epoch prefix. The queue/generation cases in
`tests/notice_reaction_dispatch.rs` use actual SDK notice frames and the physical
mail owner; they complement held-seed/reopen routes and do not alone establish a
physical reconnect. Worker alarm timing, hosted behavior and a whole-branch gate
remain separate.

## Apparatus controls

`board_store_readback_test.py` checks closed-file refusal, exact binary/null
cells, expression-index metadata, nonempty density across epochs, and refusal
of v4 keys and the retired schema. These are apparatus controls, never product
evidence. Use an isolated Python environment containing Pydantic v2 and pytest:

```sh
python -m pytest scripts/verification/board_store_readback_test.py \
  --basetemp "$BOARD_RUNTIME_PARENT/apparatus-tests" -q
```

The base-temp path is task-owned disposable output; pytest replaces it on a
repeat run. Type-check the changed files with basedpyright and run Ruff check
and format checks. The skill's whole-apparatus gates remain separate: any
inherited violation must be reported, not described as a green whole-tree gate.
