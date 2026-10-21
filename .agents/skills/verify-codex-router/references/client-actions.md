# Selected-machine client actions — U16/R16

**Draft/unobserved upkeep.** Adopt Specification R16 including its inventory
availability and shared native catalogue corrections, Program Design “U16 client
action binding” and its final native catalogue section, and Verification Design
U16/adapter/stored-policy/U16-INV-F1 and the five native catalogue proof rows.
The current reviewed canonical plan is
`tmp/plan-workflows/2026-10-19-tui-component-system-native-catalogue.md`, S2b/C1–C4.
Native uncertainty and existing managed operation readback remain distinct below.
Provider execution still requires its separate pinned observation packet. No
product edit, provider run, SSH action, policy change or target observation is
authorized by this recipe.
The Lead owns those documents and delivery acceptance.

## The boundary being proved

Client and source are captured domain choices in the one New form: existing
native Codex, native background Claude, or Host-managed Cursor ACP. Managed
Claude records stay managed. Preserve typed managed/native origin even for equal
visible IDs. A Claude background job handle is a separately labeled result;
it is never coerced into a native session UUID or managed SessionRef.

The selected source uses existing domain adapters and the fixed-program SSH
source entrypoint when remote. Source reads and one-shot mutations remain
separate. Preserve full service/endpoint/session identity, directory, capability
and generation binding; there is no local/default fallback. A managed ACP
mutation has one Router operation ID before dispatch. Lost managed output means
Unknown plus that ID and read-only operation inspection.

Native Claude launches through its existing external process owner and creates
no Router provider-operation record. Preserve its actual observed process/SSH
receipt and status with the captured client/source/action identity. Lost native
output means unknown effect: no fabricated Router recovery ID, operation-show
offer or second automatic launch. Router NotFound cannot establish that a native
launch never happened. Keep a returned vendor job handle separate from the native
session UUID. Neither managed nor native uncertainty permits automatic resubmission.

Managed Cursor New uses the existing workspace-write request and normal approval route.
New Claude in the form always uses native background; existing managed Claude
Load/Resume stays on its managed API when the endpoint/session permits it.
Managed Load/Resume instead takes directory, requestedPolicy and Approver from a
fresh exact-target Stored summary of the actual ProviderOperationStore record.
The requester is the selected source's resolved Human. These are distinct roles:
current Human must not replace saved Approver or policy. Missing summary, wrong
origin, vanished target or incompatible endpoint fails without dispatch. This is
read-before-submit preservation, not a new atomic concurrency guarantee.

## Proof cases and wrong behavior

| Case | Real path and oracle | Limit / prerequisite |
| --- | --- | --- |
| CLIENT-FORM | Actual component and public TUI: searchable Client/Machine, directory/args, one frame, query/Escape/resize preservation and one Start. Assert exact captured client/source/effect identity. | Permanent product tests and current original images awaited; changing a label alone cannot earn this. |
| ORIGIN-IDENTITY | Managed Claude, native Claude peer and managed Cursor, including equal opaque IDs across origins/endpoints, reorder and late loader results. Preserve typed origin and correct eligible action. | Labels/model-provider strings are not origin evidence; do not deduplicate unlike origins or invent a native-ID join. |
| HUMAN-LOAD-CALLABLE | Exercise the actual ConversationClient Load/MCP callable boundary with Human requester through a real owned Host to backend operation. Assert Human, target, operation ID, directory, policy and Approver unchanged. Session callers remain supported. | Lower-level ConversationLoadRequest already accepts ProviderIdentity and cannot prove the public input. A SessionRef-only installed schema cannot support this case; never fabricate identity to satisfy it. Qualify the declared new Host's actual schema. |
| STORED-PRESERVATION | Seed a real provider Session record with write-restricted policy, saved directory distinct from New directory, and saved Approver distinct from current Human. Drive actual source adapter → Stored inventory → public Load/Resume → backend operation and resulting stored record. Assert those three saved fields and caller operation ID, with source Human as requestedBy. | Only external vendor runtime may be substituted for this storage/API interaction test. A hand-built summary or direct backend request bypasses the obligation. Workspace-write/current-Human defaults and omitted requestedPolicy must fail. |
| INPUT-SCHEMA | Generated client/MCP types expose Human-or-Session Load input. HostedProvider summary requires requestedPolicy; native Claude summary does not acquire managed-policy fields. Cut over callers/fixtures/schema consumers. | Requalify the declared Host's actual callable schema; source declarations alone do not prove the installed API. These are the two admitted public read/input projections, not a persisted-schema or policy change. |
| EXISTING-REJECTIONS | Human on Codex-only branch rejects before native effects; wrong-service Session requester/Approver rejects at client validation; wrong target service/binding and stale generation reject through existing provider admission. Source adapter rejects absent exact Stored target or wrong origin before submit. | Do not assert OS-owner authentication or wrong-policy rejection by the server: the governing correction adds neither. The adapter must preserve its actual source-resolved attribution. |
| CURSOR-LOCAL | Real public agent-sessions TUI → actual owned Host → actual installed Cursor managed create without prompt, and supported Load/Resume of its real SessionRef. Observe actual capabilities and settlement. | Fake ACP peers support lower-layer wiring only. Runtime/auth availability is separate from model inference and must be qualified. Unsupported Fork has its real reason and no effect. |
| CURSOR-REMOTE | Same real managed create/Load/Resume on selected configured source through fixed-program SSH, new declared binaries on both machines, actual hostname/cwd/SessionRef, source Human and saved metadata preservation. | Explicit isolated registry/executable and remote runtime grant awaited. MCP-only inventory profiles cannot silently gain launch fallback. No arbitrary shell/program request or credential copying. |
| CLAUDE-LOCAL-REMOTE | Reuse R15 native launcher on selected local/remote source, no initial prompt; actual background receipt/owned job and supported agents JSON readback. Resume/Fork consumes only actual native session identity. | See claude-background.md. Substitute process receipts do not establish vendor registration; preserve job handle versus conversation UUID. |
| MANAGED-UNCERTAIN-ONCE | Real source/client boundary loses managed ACP output after one submission, repeated Start/input while pending, missing operation result, and stale source/binding. Require one Router operation ID/submission, truthful Pending/Unknown and read-only reconciliation. | Substitute only the external failure; retain actual operation admission/identity/dispatch under test. Never retry uncertain mutation to obtain a nicer screenshot. |
| NATIVE-LOST-RECEIPT | Drive actual native child creation/process or fixed-program SSH transport, lose launch output after the child can have started, then repeat input. Preserve captured client/source/action and unknown effect; assert no Router recovery ID or operation-show offer and exactly one launch. Also keep job-handle and native-session-ID evidence distinct when output survives. | Only external vendor behavior may be substituted for this wiring check. Router NotFound is no evidence of native non-launch. This does not establish actual vendor registration/grammar, which needs the separately qualified runtime journey. |

## Independently available inventory parts

Managed Stored rows and native Claude live/Loaded coverage are separate observed
parts. For the same captured source/query/epoch/binding, retain valid Stored rows
when only the added native-live read is unavailable. Require a visible partial
notice naming missing native inventory, including when the missing part supplied
zero rows. A successful empty live response is complete and has no unavailable
notice. Do not reuse native Codex runtime coverage as this independent state,
infer that a missing live record deleted a session, or infer launch capability
from catalogue completeness. Preserve captured Details/action identity and equal
opaque IDs across distinct native/managed origins.

Native discovery is qualified independently of managed ACP. Read the exact
current service/epoch endpoint inventory: canonical `claude-local` membership
qualifies a native candidate even when its availability is Unavailable and it
has no channels. An absent endpoint supplies no candidate; wrong service/epoch,
duplicate membership or malformed inventory rejects. Never manufacture an
endpoint, infer native membership from a label/runtime kind, or turn membership
into launch capability.

Observe the actual request boundary, not just returned row labels. Managed
Stored/All reads require the qualified ExternalProvider channel; native Claude
reads use Stored/Interactive for background history and Loaded/Interactive for
live peers plus jobs, independently of a managed operation store. Captured native
actions revalidate against the appropriate native Stored/Loaded evidence.
Loaded/All followed by origin filtering fails the obligation: equal IDs and
timestamps at a cursor boundary may hide a native row. Native responses must
contain only the exact native origin/endpoint; managed responses only their
managed origin. Preserve equal IDs across partitions and reject duplicates within
one partition. Keep current folder/source/provider/query guards, request binding
and total budget; report unsupported combinations rather than rewriting queries.

The deciding path is actual local API or configured HTTP MCP/fixed SSH source
read → classifier → loader/continuation → reload publication → rendered notice.
Keep those owners real; substituting an already classified loader result cannot
prove the read-boundary distinction. Product tests stay with the Implementer.

| Case | Required deciding outcome |
| --- | --- |
| INV-AVAILABLE-PARTS | Actual Host/store Stored success plus published native-live unavailable/discovery error retains the qualified managed rows and shows explicit partial coverage. Include a valid live prefix followed by unavailability and existing-budget expiry. |
| INV-EMPTY-COMPLETE | Successful empty live read stays complete; no unavailable notice or invented rows. Equal native/managed IDs remain distinct, with same-origin duplicates rejected. |
| INV-FAILURE-BOUNDARY | Before HTTP/SSH errors lose their category, only timeout/transport/discovery or published code -32050 with kind unavailable and stage discovery can mark the extra live part incomplete. Invalid parameters/cursor (-32602), unsupported/wrong source or epoch, oversized/malformed/mismatched/unknown errors still reject the affected source. Stored failure never becomes a successful catalogue. |
| INV-NOTICE-IDENTITY | Exercise actual scheduler/publication and component state; retained row/selection/captured action survive partial refresh and resize. Complete versus partial is visible. Private sentinel text/data in server failures must never reach presentation or error output; retain only fixed categories. |
| INV-NATIVE-INDEPENDENT | Real service/client with canonical channel-less Unavailable `claude-local` plus independently attached native registry renders its native row without managed ACP/store. Actual query is Loaded/Interactive; absent endpoint, wrong service/epoch and duplicate membership are negatives. Check default and configured publication paths. |
| INV-PARTITION-PAGE | Equal managed/native IDs and timestamps at a real page boundary both survive separate Stored/All and Loaded/Interactive reads. Wrong-origin pages and within-partition duplicates reject; query/continuation/source/epoch/budget remain pinned. Actual native target validation uses the native-only query. |

Use the existing local/configured integration owners and classification tables,
then current compiled public-entry/capture evidence. These cases prove graceful
composition, not native registry availability, provider compatibility or actual
local/remote vendor effects. The approved native summary/coverage projection
below is a separate coordinated read-contract change; these older cases add no
other storage, authentication, retry or transport authority.

## Shared native background catalogue — C1–C4

These lasting cases adopt the final R16 and Program/Verification Design catalogue
sections. Implementation and the new frozen Host/CLI target are awaited. Extend
the existing client action/TUI route; no direct TUI vendor scanner, parallel
renderer, new server or Router native-history store is permitted. The source-local
reader uses the supported fixed `claude agents --json --all` read, normal home
and an owned bounded child. It never prompts, attaches, resumes or stops a job
during discovery. A previous Host missing a job is historical negative evidence,
not proof for a new Host.

| Case | Deciding observation and wrong behavior |
| --- | --- |
| NATIVE-JOB-DECODE | Real child/parser with a controlled external executable: one mixed public list includes interactive, job-backed background and peer-only background lacking both job id and lifecycle state. Retain valid jobs; leave peer discovery to its registry. A partial job discriminator rejects; missing only native UUID gives explicit unidentified coverage. Check duplicate/conflicting identities, absolute cwd/time, unknown state, output/time bounds and exact child settlement. A kind-only decoder must fail the mixed valid list. |
| NATIVE-SERVICE-PROJECTION | Actual new Host/service/API/client with managed SQLite, peer fixtures and real job-reader child: Stored terminal history, Loaded live jobs/peers, Active working only. Corroborate one exact UUID+directory once; preserve equal managed/native UUID and time across pages. Never invent peer fields or Updated time from started time. Wrong endpoint/service/epoch/cursor and ambiguous claims reject. |
| NATIVE-INDEPENDENT-COVERAGE | Carry serialized peer/job coverage through default/configured loaders, scheduler and mounted notice. Either native source unavailable retains other qualified observations with an explicit missing part; successful-empty is Complete. Invalid job output stays invalid and cannot enable actions. Check timeout after a qualified prefix, not a preclassified result above the decoder. |
| NATIVE-CAPTURED-ACTIONS | Actual read adapter/revalidation and existing action owner keep UUID, cwd, source and typed origin through refresh, reorder and partial failure. Job handle differs from native UUID. A stopped native Stored job may resume in background; existing interactive peers and managed saved policy/Approver remain. Require one dispatch; lost output gives no retry or fabricated Router operation. |
| NATIVE-ACTUAL-BOTH-MACHINES | On separately pinned new isolated debug Hosts/CLIs, public native New with no prompt returns an actual job and native UUID; that same Host catalogue and compiled TUI show it before any prompt. Inspect/Copy, stop only the owned job, observe Stored history and supported background Resume/Fork. Repeat selected-source semantics on Sunbook, preserving production/profile/credentials. Actual managed Cursor create/Load/Resume and all earlier menu/grid/resize images remain required. |

Keep job handle, native conversation UUID, peer process and managed SessionRef
distinct. Working is active; blocked is waiting; done/failed/stopped are terminal;
unknown grants no classification-dependent action. Peer-only rows never acquire
invented job fields or unidentified-job counts. Native Stored and Loaded reads
may corroborate the same UUID across views; duplicates within one stream still
reject. The public catalogue response does not prove TUI key/action behavior.
Retain original native Details/result/source images and exact source/build/vendor/
grid/action/cleanup attribution. Actual vendor and two-machine observations,
matching compiled images and any still-missing lasting driver remain explicit gaps.

## Decisive current source anchors

Read these again against the declared final source, not this draft's assumptions:

- collaboration-client `conversation_client.rs`: ConversationLoadInput carries
  ProviderIdentity; `conversation_session_operations.rs` validates Session
  locality, rejects Human on Codex Load and forwards the managed request. An
  omitted Approver defaults to requester, so the source adapter must supply the
  saved Approver explicitly.
- collaboration-protocol `provider_conversation_contract.rs`: lower provider
  Load already takes ProviderIdentity and ProviderRequestedPolicy;
  `provider_session_list.rs`: HostedProvider requires requested_policy while
  native Claude peers remain a separate variant.
- collaboration-service `provider_session_record.rs`: actual stored record owns
  directory/policy/createdBy/Approver; `provider_session_inventory.rs` constructs
  HostedProvider including requested_policy from that same record.
- collaboration-client `owner_identity_resolution.rs`: established source Human
  resolver uses bounded `/usr/bin/id -un`, not a manufactured SessionRef.
- agent-collaboration `source_ssh_protocol.rs` and `source_ssh_transport.rs`:
  existing bounded typed private request and fixed-program one-shot SSH carrier.
  SourceOperation::ProviderAction carries managed operation identity;
  source_provider_actions.rs owns fresh Stored metadata and public managed calls.
  The native process outcome remains a separate proof obligation.

## Delivery evidence and open grants

Before any run, bind the admitted canonical plan, exact local/remote source and
binaries/hashes/build commands, isolated owned Host roots/ports/registries,
supported installed clients/versions, provider/auth/no-prompt scope, source
identity/endpoint inventory, expected capability, and exact owned cleanup.
Preserve normal Codex home and production processes/configuration.

Return real commands/exits, API schema and stored-field readback, actor/target/
managed operation IDs, native observed receipt/status and captured source/action,
actual local/remote settlement, original Client/Machine/result/
managed Details/unsupported-state screenshots, viewport and source/build with
limits. Keep component, API/storage, process substitute, actual vendor and
two-machine results separate. No observation, acceptance or new runtime driver
is supplied here. Assign any required lasting native/provider driver path before
authoring it; the existing TUI capture wrapper does not perform provider actions.
