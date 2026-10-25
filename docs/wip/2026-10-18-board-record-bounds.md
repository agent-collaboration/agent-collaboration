# Board record bounds: source inventory and measurements

Status: implementation in progress under PD1 E29 at `ecd4888d`, with the key-error correction at `37f3eacd`. The caller-limit table and trail limits are adopted. Shared domain validation is implemented; native integration and maximum-kind proof remain pending. The governing record budget is **1,048,576 encoded bytes**; the schema budget is **262,144 bytes**. Trail limits remain 32 tags, 16 links, a 1 KiB repository path and a 2 KiB URL. The earlier 512 KiB budget was explicitly superseded after measurement.

The Lead withdrew the proposed backlog gate and 1,000-effect cap before either was implemented. Project accepted ledger effects become a bounded count/digest header and are rebuilt from historical request-tagged feed metadata; Fleet retains ordered receipt pairs for the ledger's lifetime because its feed is pruned and no-ops return old positions. Write replies carry a first page of at most 256 pairs, total count, canonical digest and continuation cursor; request_effects retrieves the rest. A5 rejected tails become a header plus complete membership rows, with distinct record and membership digests and atomic completion. Refusal evidence retains a 256-entry ordered prefix, full count and canonical SHA-256 digest; abridged cycles are prefixes of a closed witness, and assignment lists are non-exhaustive. Subtask keys are capped at 8 KiB hex through typed constructor/between errors. There is no production `between` caller, so no recorded key-error variant is authorized. Final size proofs must use maximum metadata on every repeated value. These are governing changes, not claims that implementation or proof is complete.

The source baseline is description commit `9583f339` plus the permanent size-test working changes. Native measurements use the pinned library `01d846b2`, real project writes, strict current-row readback, actual snapshot `Vec<SqlValue>` rows, JSON codec roundtrip and exact history-content equality. They are constructed stress cases, not observed production volumes. The tail and accepted-effect fixtures do not claim an actual takeover or full admission fanout; the cycle and ambiguous-subject fixtures now call their real domain rules, and the ordering key comes from repeated `SubtaskPlace::between` calls.

## Observed native bytes

| Case | Encoded bytes |
| --- | ---: |
| 64 KiB post text, 64 references, maximum actor/acting-for identities | 355,237 |
| 16 KiB description containing NUL bytes, maximum actor | 144,942 |
| 64 KiB trail text, 32 tags, 16 links, 4,096-byte escaped repository and 1 KiB path | 900,646 |
| Same trail, 8 links | 613,014 |
| Same trail, 6 links | 541,106 |
| Same trail, 5 links | 505,152 |
| Same trail, 4 links | 469,198 |
| Reconstructed project schema, 47 statements | 12,381 |

These observations use native fixture positions, times and origin metadata. Final maximum-kind proof must also account for the maximum fixed metadata, nested variants and the remaining caller limits; this table is not that completed proof. The first attempt failed before measurement on a missing owning-thread FK. The corrected 512 KiB assertion reached the intended trail-size failure. The Lead retained 16 links and raised the governing bound to 1 MiB. The three initial record cases and schema then passed under that revised contract, not because product code changed.

## History-grown values

All values survived current-row and immutable feed readback without truncation. The corrected producer-rule run passed 1/0, exit 0, with all 3,862 source hashes unchanged. An earlier native fixture used Reviewer assignments, which the subject-selection rule excludes; only the corrected Implementer measurements below are producer-relevant.

| List/key | 1,000 items | 10,000 items | 20,000 items |
| --- | ---: | ---: | ---: |
| Rejected-tail request IDs | 71,420 | 440,420 | 850,420 |
| Dependency-cycle path (includes the closing node) | 71,483 | 440,483 | 850,483 |
| Subject-required eligible assignments | 110,436 | 830,436 | 1,630,436 |
| Accepted decision's references and positions | 296,284 | 2,699,285 | 5,379,285 |
| Subtask key after repeated dense insertion | 30,548 | 32,798 | 35,298 |

The corresponding subtask keys encode to 250, 2,500 and 5,000 hex bytes. The whole-row measurements include the maximum actor fixture; the accepted-effect position origins use the normal fixture label. A project with 10,000 tasks or active workstreams is a deliberately heavy possible case, not a measured deployment profile. The 10,000-effect ledger already exceeds 2 MB; the 20,000-assignment ledger exceeds the 1 MiB board rule. No cap or truncation was applied to obtain these observations.

| Value and decisive source | Practical growth and principled bound | Could separate rows preserve it? |
| --- | --- | --- |
| Tail IDs: `fact_records.rs:137`, `board_payloads.rs:37` | The set is every rejected request beyond the cutoff. Ack lag, lease/provisional lifetime and write rate affect its size. There is no useful per-tail count rule; global position/identity and physical limits do not establish 1 MiB. A5's real report producer is still owed. | Yes: a header/digest plus membership rows. Completeness must be explicit before RC-4 treats an absent ID as fresh. Report transport, fact shape and completion semantics need design; this is not a validation-only change. |
| Cycle path: `work_rules.rs:119–198` | A* returns a path through the project's dependency graph and its closing source. Its size grows with project tasks; there is no per-path policy limit. | Ordered evidence rows plus a bounded refusal reference can preserve every node and its order. Persisted refusal and read/replay contracts would change. |
| Ambiguous subjects: `work_rules.rs:90–114` | The rule returns all eligible Lead/Implementer/Verifier assignments. An actor can hold eligible roles across many workstreams. Cardinality per workstream does not bound the actor's total. | Existing role records could supply a decision-position-anchored set, or dedicated membership rows could retain it. Historical stability and exact retry replay must be designed before replacing the inline list. |
| Accepted effects: `project_admission/mod.rs:215–305`, `operation_context.rs:300–386` | Every planned record and resolved notify intent is appended to both lists. Role resolution iterates all holders; caller targets can add recipients too. No per-decision effect cap was found. | The feed already has per-row request IDs, record IDs and positions. A bounded header plus indexed request-scoped effects could avoid duplication after proving the exact membership/order invariant. API replay and remote retrieval still need a settled contract. |
| Ordering key: `subtask_order.rs:12–105` | Dense insertions grow the byte vector; the constructor has no maximum. CPU/memory, ingress and global feed limits are practical ceilings, not a useful per-key bound. | Splitting bytes alone does not preserve the current single-column lexicographic order/keyset behavior. Fixed-width ranks/rebalancing or another order structure would change the “rewrite only this subtask” model. No cap or rebalancing is implemented. |

## All 25 Record variants

Common envelope strings are finite: UUIDs/digests, restricted 64-byte EndpointId, 4,096-byte HumanId/SessionId and 256-byte MachineLabel. Time/numeric domains are finite. SubjectLine is at most 120 characters. Post references are already at most 64 distinct fixed-ID targets.

| Record kind | Remaining variable field or nested value |
| --- | --- |
| Workstream | `goal`, closed `note` |
| Task | `body` |
| TaskLinkEvent | No unbounded field found |
| Subtask | History-grown `SubtaskPlace` |
| WorkTrailRow | Approved tag/link/path/URL limits; enforcement owed |
| TrailRedaction | No unbounded field found |
| RoleAssignment | `RoleEnd::StoodDown.reason` |
| OwnerGrant | `ReplaceLead.reason` |
| StandDownAcknowledgement | No unbounded field found |
| Move | `evidence`, `reason`, rejected reasons/guard names |
| Fact | Receipt, evaluator, hook, scope and lifecycle values below; derived tail set |
| Project | Existing Description/StatusName/definition-reference bounds |
| PlanRevision | Shared EvidencePointer path/URL bounds |
| ReadMark | Finite; this is Router-local, not an admitted project-feed write |
| NotifyIntent | Delivery refusal `detail` |
| SubscriptionMembership | Existing validated identity/scope/policy/time domains |
| Request | Derived accepted lists and refusal evidence; recursive ProjectRejected shape |
| Post | Existing 64 KiB PostText/64 references and finite identities |
| Thread | Finite subject and two holder identities |
| ThreadRefiled | Two finite subjects |
| Workspace | `repositories` count; each locator already at most 4,096 bytes |
| Board | Initiative `goal`, `done_when` |
| Adr | `repository_files` count and ArtifactRef path |
| ProcessDefinition | Exact source, edge/guard lists, source-to-parsed-graph consistency |
| DirectoryEntry | Existing title/identity bounds |

Fleet variants and local-only ReadMark must be sized through their owning representation; a project-store test must not pretend to admit them as project records. Final reporting will distinguish that from actual native project-feed row measurement.

## Adopted caller limits, integration proof pending

All text values below are UTF-8 byte limits. These preserve current text/control semantics; no normalization, truncation, paging or SQL CHECK is proposed. The intended implementation home is shared domain validation, used by the existing admission and authoritative decode boundaries. The final encoded maximum test must verify the combined limits before a commit can claim this complete.

| Field/group | Proposed limit | Reason |
| --- | --- | --- |
| Workstream goal and closed note; Task body | 64 KiB each | Align with existing large board text; the workstream's two worst-escaped values still leave room under 1 MiB |
| Role stand-down and owner-grant reasons | 64 KiB | Retain the same large-text capacity beside finite identities |
| Initiative goal and done_when | 32 KiB each | Two planning fields coexist with a 16 KiB Description; simultaneous 64 KiB unrestricted values would consume the budget |
| Workspace repositories | 16 | Preserve existing 4,096-byte locators while bounding the replicated workspace row |
| ADR repository files | 16 | Bound full file references alongside existing 64 KiB MarkdownText |
| ArtifactRef.path and EvidenceRef::Artifact.path | 1 KiB | Reuse the approved evidence-path budget; repository locator remains 4,096 bytes |
| Fact.links | 64 | Match the existing post-reference scale; keep detail in separate referenced evidence |
| Receipt obligation / scenario_run / observed | 4 KiB / 256 B / 8 KiB | Obligation and outcome are summaries; raw logs remain artifacts |
| Receipt commands / command text | 16 / 1 KiB | Enough distinct gates and normal command lines without storing raw transcripts |
| Receipt artifacts | 16 | Align with the evidence-link count; 1 KiB paths and existing locators remain included in combined sizing |
| Evaluator guard / engine | 256 B each | Align with named guards and existing ResourceName scale |
| Evaluator evidence | 16 | Retain full evidence references within the row budget |
| Hook targets and scope-ack paths | 64 entries, 1 KiB each | A bounded set of affected paths, at the shared evidence-path scale |
| Scope-ack reason | 16 KiB | Explanation rather than embedded logs |
| Lifecycle closed/dropped/thread-resolution reason | 64 KiB | Preserve existing DropReason/large-text capacity; nonblank semantics remain unchanged where already required |
| Lifecycle phase name | 256 B | Existing named-entity scale |
| Move reason / evidence | 16 KiB / 16 entries | Explanation plus references; allow large evidence sets without embedding logs |
| Process source / edges / guards per edge | 64 KiB / 64 / 8 | A substantial workflow while bounding parse shape; the stored graph must equal parsing the exact source, so independent inconsistent maxima are invalid |
| RejectReason guard | 256 B | It names a registered guard, already a 256-byte ResourceName |
| InvalidShape field / delivery-refusal detail | 256 B / 16 KiB | A field name and an error explanation, rather than unrestricted transcripts |
| Recursive Refusal::ProjectRejected | At most one wrapper | No recorded producer of this legacy wrapper was found at this head; unbounded nesting adds no terminal-rejection information. Owner adoption still required. |

Derived list bounds are deliberately absent from this table. Process guard limits may bound the count of guard refusals generated by one move, but are not permission to truncate any existing derived decision evidence.

## Still required

Lead decisions on candidate caller limits and every history-grown representation; all-kind and nested-variant maximum fixtures; maximum fixed-metadata qualification; admission and stored-decode refusal controls; final Clippy/format/metadata/native execution; scoped commit. No acceptance or delivery claim follows from these measurements.
