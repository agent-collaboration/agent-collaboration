# Fleet query provenance and remaining read-cost work

Compared Central `ef87b73d301819b1e5fda9907bce27e45c5fac72` with its main ancestor `fb3cf6b5a4624ce725cb190d25099b850f95ba88`. `git show` and `git blame main` establish that all four outer hydration loops originated in `be7e61e8bc` (Fleet 0.1.102), before C6. C6 added child and refusal-tree queries to the canonical reader, amplifying the existing snapshot and import loops. The bounded repair covers that amplification (Q2/Q4); Q1/Q3 remain follow-ups.

| Audit row | Provenance and disposition |
| --- | --- |
| Q1 Directory | Pre-existing and unchanged statement count: one key query plus project, board, copy-summary and lease reads per project. Remains deferred. |
| Q2 Snapshot | Pre-existing five owner-key queries and per-record hydration loop. C6 increased each Workspace/ADR hydration from one to two statements and rejected Request hydration from one to three. Repaired with one set read per selected physical table and the same strict decoders. |
| Q3 Archive | Pre-existing project loop reads leases/reservations per project, report loop reads the canonical project per report, and workspace loop repeats those readers per board. C6 changed decoding without increasing statement count. Remains deferred. |
| Q4 Import inputs | Record/ImportDigest/name/parent read duplication predates C6. C6 amplified it through Workspace/ADR child reads. Repaired by resolving each requested name namespace in one indexed set query, sharing one canonical batch across repeated references, and preserving the original input/output order. |

## Measured regression proof

The permanent [snapshot tests](../../crates/collaboration-central-core/src/fleet_snapshot_read_test.rs) observe actual SQLx statement events inside real owning Fleet transactions. Fixture writes and transaction bookkeeping are outside the observation. Counts are statements, not latency or rows scanned.

| Workload | Before, sizes 2 / 8 | After, sizes 2 / 8 |
| --- | --- | --- |
| Q2 snapshot of Workspaces with ordered children | 9 / 21 | 7 / 7 |
| Q4 bundle with Record, ImportDigest and name keys per Workspace, plus one missing reference | 15 / 57 | 3 / 3 |
| Mixed Workspace/ADR/rejected Request snapshot, 2 / 8 records per kind | Not measured before | 12 / 12 |
| Same mixed kinds through paired Record/ImportDigest bundle keys | Not measured before | 7 / 7 |

Both first regressions were observed failing on the pre-fix readers for those growing counts. Their expected fixed budgets passed after batching. The tests compare exact bodies, ordered repositories/files, original import digests, nested refusal trees, absence and snapshot continuation boundaries. Separate real-store corruption cases require both page and bundle readers to reject invalid repository tags, blob file paths and noncontiguous reason-child ordinals. The existing canonical codecs still own all physical-class, count, identity, scope and variant validation.

```sh
cargo nextest run --locked -p collaboration-central-core --features conformance \
  -E 'test(fleet_snapshot_read::tests)' --no-fail-fast --success-output immediate
```

## Deferred follow-ups

- **Q1:** `crates/collaboration-central-core/src/fleet_directory_read.rs:48`. Batch project, board, copy-summary and lease hydration. Preserve identity/scope checks, typed projection validation and filtering semantics. The unchanged full successful hydration cost is `1+4N` (9 statements for two projects, 33 for eight), derived from source rather than instrumented in this repair.
- **Q3 project/report inputs:** `crates/collaboration-central-core/src/fleet_archive_snapshot.rs:40` and `:114`. Batch lease/reservation reads per selected project range and canonical project reads per report range. Current source costs are `1+2P` and `1+R`, respectively; retain archive predicates and missing/wrong-parent corruption checks.
- **Q3 transitive workspace loop:** `crates/collaboration-central-core/src/fleet_scope_snapshot.rs:95`. Workspace archive inputs repeat the board readers above. Evaluate together with Q3, rather than silently changing its archive/admission behavior in the Q2/Q4 repair.

Retention policy, revocation/admission changes and library journal maintenance remain separately owned. This local query repair establishes no hosted performance result or production acceptance.
