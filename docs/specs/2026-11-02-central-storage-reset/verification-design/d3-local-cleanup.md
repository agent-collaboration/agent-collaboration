# Verification Design: D3, local cleanup

How D3's requirements will be shown to hold. A plan: nothing here has been observed. Upstream: [R23–R29](../specification/local-storage.md), [program design](../program-design/d3-local-cleanup.md).

**Target for every row:** a debug Router built from the D3 branch, each with its own debug root, upgraded from **populated** D1 stores, never empty ones. The fixture is a debug root that ran D1's journeys (O1, O9, O11) plus approvals, a question, provider operations on `codex-local` and `claude-local`, an enrollment, and a snapshot install. It's frozen as a copied debug root in the verification harness, not committed. Luna sessions on the debug Router only; the Claude provider runs on Haiku after asking the owner. A local Tokio central serves the enrollment. Staging isn't needed: D3 changes no central behavior.

## Observations

| ID | Observation | Decides |
| --- | --- | --- |
| <a id="q1"></a>Q1 | **Schema readback after upgrade.** Open every scoped store and list every table, column, key and CHECK. Every Router table matches its concern prefix. Every table, Router and library, has a UUIDv7 key and both timestamps, apart from SQLx migration tables, which match SQLx's own shape. The only CHECKs are boolean, and the legacy Mailbox tables are absent | R23, R24, R25, R29 |
| <a id="q2"></a>Q2 | **Row readback after upgrade.** For every rebuilt table, the row count and each row's business columns equal the pre-upgrade copy. Every new key is UUIDv7, and every timestamp is valid. Hook migration history keeps every version, with its checksum as bytes. A seeded invalid stored value (a non-UUIDv7 key, or a timestamp of `0`) is refused on read with a typed error | R24 |
| <a id="q3"></a>Q3 | **Behavior is unchanged.** After upgrade, the existing approval, question, `message send`, `wake` and provider-session journeys pass as before, on the upgraded root | R23 |
| <a id="q4"></a>Q4 | **Kind-aware removal.** **needed:** the D1 debug mail clock (`debug_mail_clock.rs`) on the Router. Seed a copy of each message kind (direct, wake, schedule run, board activity) plus approval and question records. At day 59 all are present; after day 60 the message copies are gone, and approvals and questions remain until today's two-calendar-month rule removes them | R26 |
| <a id="q5"></a>Q5 | **JSON files moved.** After upgrade and restart: `fleet/enrollment.json`, `machine-session/state.json`, `machine-notice-progress.json`, `snapshot-install-attempts/*`, `service-identity.json` and `approval-routes.json` are gone, and their state reads back from the new tables (enrollment active, resume cursor kept, the in-flight install attempt reused rather than minted anew). Only `ports.json`, `providers.json`, `service.json` and schema bundles remain | R27 |
| <a id="q6"></a>Q6 | **Machine id switch with crashes.** **needed:** named crash points between every pair of stores in the switch, and before and after the service identity file is retired (debug builds only). Kill at each point and restart: the switch finishes. Pending approvals and known provider operations are intact, stored Messages verify their digest, and the manifest publishes the machine id. A new send to an address with the old id is refused with "this machine's id is now …". A message queued before the switch ends as "expired" | R28 |

## Rows

| Requirement | Proven by | Reference | Passes when | A wrong build it catches | Unverified |
| --- | --- | --- | --- | --- | --- |
| U11 | Q1, Q3 | [R23](../specification/local-storage.md#r23) | every Router table has its prefix; the approval and question journeys pass unchanged | a table left with an old name; a rename that breaks approvals | none |
| U7 (local) | Q1, Q2, Q6 | [R24](../specification/local-storage.md#r24), [R28](../specification/local-storage.md#r28) | every key is UUIDv7 with timestamps; rows are preserved; the switch completes at every crash point | an upgrade tested only on empty stores; lost or coerced rows; a half-switched Router | ids inside Codex, Claude or other tool records (theirs, R24 excludes them) |
| U13 | Q1 | [R25](../specification/local-storage.md#r25) | only boolean CHECKs | `CHECK(singleton = 1)` kept | none |
| U9 (local) | Q4 | [R26](../specification/local-storage.md#r26) | message copies gone after 60 days; approvals and questions on their own rule | one clock for every kind; a second removal pass | none |
| U12 | Q5 | [R27](../specification/local-storage.md#r27) | exactly the listed files remain, and moved state survives restart | a moved file still read; a new install-attempt identity | none |
| U11, U7 (library) | Q1 | [R29](../specification/local-storage.md#r29) | the Mailbox tables are gone | the tables left in place | none |

Every observation runs on Tokio only; D3 has no Cloudflare surface.
