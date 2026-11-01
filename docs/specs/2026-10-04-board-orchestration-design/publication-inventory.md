# Published board-design decisions

This publication carries the owner's settled meaning into the current board design and preserves newer `main` content. It changes no product code. Authority and implementation proof are separate: publication supplies no runtime or release receipt.

## Labels and scope

| Label | Meaning and coverage |
| --- | --- |
| owner-settled | A requirement cites the owner's decision. Later decisions named below govern superseded clauses. |
| Lead realization | A derived requirement or structural/proof binding. All otherwise unlabelled statements in the Program and Verification Designs carry this label; they are not new owner decisions or passing observations. |
| superseded | An earlier clause retained for context. The named later decision governs. Existing 0.1.102 cut notices remain verbatim. |
| open | Questions and gaps explicitly marked open in the current requirements, specifications and open-questions document. This publication settles none. |
| planned-but-unbuilt | Central automation definitions/firing, grid, notification cadence and remote-session changes in the central-automations design. S9 publishes their contract; the other slices and gates establish their implementation. |

## Governing decisions and source checks

| Statement | Label | Current home and effect | Evidence inspected for this publication |
| --- | --- | --- | --- |
| Lease expiry fences project writes, never automations | owner-settled | U38; WA-11; FL-9. PD2 removes the automation activity gate, and the proof rows require automations to keep running. | No new runtime claim. Central-automations V2 owns the firing observation. |
| Local-first is online work; creation requires central first | owner-settled | U80; RC-4; WA-5/6/10; FL-8/10. Unsent writes are not carried over, served or reported. Unknown retries run fresh; no rejected-tail entity or voided/unresolved request family remains. | `collaboration-service/src/project_admission/project_creation.rs` submits the directory before local completion; `message-board/src/request_decisions.rs` defines the typed current outcomes. Source evidence only. |
| The old holder's file remains on disk, unread | owner-settled | U80; WA-6; PD2's registry transition. Central's transfer record is the sole takeover record. | Detailed shared-reader refinements in the transfer remain Lead realizations; this slice does not assert their execution. |
| Notification timing supersedes immediate local notifications | owner-settled; planned-but-unbuilt | Central-automations U14/R6n replaces U63's timing: one delivery per recipient per 10 minutes. Direct messages, wakes, runs, approvals and questions stay immediate; the remote replication gate remains. | V7/M6 own the real recipient observation. Historical immediate-notification statements are explicitly superseded. |
| Wake timing has 5-minute precision and repeats no faster than 10 minutes | owner-settled; planned-but-unbuilt | Central-automations U12/U13 and R6g/R3 replace U66's former 5-minute default. Other lifetime/slot rules remain. | V3/V5/V6 own the entry and central checks. |
| Messages, pushes and interaction history belong to local Turso interaction.sqlite | owner-settled | PD4 diagrams, storage bindings and §3c agree with the owner decision. Automation owns its definitions, runs and owed push handoff. | `interaction-storage/src/lib.rs` exports InteractionStore and InteractionAtomicStore with push/history/worker modules. Source evidence only; no migration or durability rerun. |
| Remote wakes currently mail a request to the recipient that owns the timer | Lead realization; existing source | PD4 names the current typed boundary, no longer an open M4 question. Central-automations S6 plans its replacement. | `collaboration-service/src/collaboration_application/wake_operations/remote.rs` constructs WakeBody/RemoteWakeRequest and sends wake mail. Source evidence only. |
| Process moves, project stage and three hook tools were removed for 0.1.102 | superseded | Preserve later main cut notices in spec 1 and PD1–PD3. The older transfer cannot restore them. | Current main text retained byte-for-byte. |
| Requested public message cancellation is retired | superseded | Preserve newer main retirement text and provisional automatic-cancel behavior in spec 3 and PD4. | Current main text retained byte-for-byte. |

## Requirement inventory

Every U row in the curated requirements is listed below. Its exact source and partial supersessions stay in the linked requirement's authority cell. An owner-settled label describes authority, not implementation status. Explicitly open questions retain the open label.

| Requirement | Label | Subject | Source |
| --- | --- | --- | --- |
| U1 | owner-settled | The environment proves the work, so the owner's attention goes to design rather than to phase control and r... | [requirements.md](requirements.md), including later qualifications |
| U2 | owner-settled | Any agent or the owner can learn what is true now about a piece of work without replaying the log or trusti... | [requirements.md](requirements.md), including later qualifications |
| U3 | owner-settled | Keep the existing structure and rename two levels | [requirements.md](requirements.md), including later qualifications |
| U4 | owner-settled | Milestones, tasks (with one level of subtasks) and ADRs are entities | [requirements.md](requirements.md), including later qualifications |
| U5 | owner-settled | A task's status changes only through a recorded move; dependsOn forms a DAG and cycles are rejected; a task... | [requirements.md](requirements.md), including later qualifications |
| U6 | superseded | A milestone is a thin delivery checkpoint (name, doneWhen, open → in_review → approved → merged, links to t... | [requirements.md](requirements.md), including later qualifications |
| U7 | owner-settled | Authority sits on entities | [requirements.md](requirements.md), including later qualifications |
| U8 | owner-settled | Router owns the record shapes, in Rust; skills and repos supply content validated against them | [requirements.md](requirements.md), including later qualifications |
| U9 | owner-settled | The board is fabric-compatible | [requirements.md](requirements.md), including later qualifications |
| U10 | owner-settled | Every identity Router mints is a UUIDv7; dedup uses business keys, never another UUID version | [requirements.md](requirements.md), including later qualifications |
| U11 | owner-settled | Decisions are server-first; facts are local-first and shipped as appends; local-only state never leaves its... | [requirements.md](requirements.md), including later qualifications |
| U12 | owner-settled | ADRs are immutable | [requirements.md](requirements.md), including later qualifications |
| U13 | owner-settled | Work items can be spikes or builds | [requirements.md](requirements.md), including later qualifications |
| U14 | Lead realization | The board hands notices to interactions and never delivers itself; automations act through interactions | [requirements.md](requirements.md), including later qualifications |
| U16 | owner-settled | A project's Lead and helpers run on one machine | [requirements.md](requirements.md), including later qualifications |
| U17 | owner-settled | The renames reach the API and CLI too, cut over in one release | [requirements.md](requirements.md), including later qualifications |
| U18 | owner-settled | A subtask is its own record | [requirements.md](requirements.md), including later qualifications |
| U19 | superseded | A project that doesn't need delivery checkpoints uses a default milestone instead of having none | [requirements.md](requirements.md), including later qualifications |
| U20 | owner-settled | Fleet-level entities (workspace, board, registry, leases) are written only on the central fleet server | [requirements.md](requirements.md), including later qualifications |
| U21 | owner-settled | Each project's store has exactly one writer at a time, the machine holding its lease | [requirements.md](requirements.md), including later qualifications |
| U22 | owner-settled | Use Turso (R14) | [requirements.md](requirements.md), including later qualifications |
| U23 | owner-settled | A project is one unit | [requirements.md](requirements.md), including later qualifications |
| U24 | owner-settled | The central server holds the project metadata needed to find, search and know what projects exist | [requirements.md](requirements.md), including later qualifications |
| U25 | owner-settled | One database per project | [requirements.md](requirements.md), including later qualifications |
| U26 | owner-settled | Stack rule | [requirements.md](requirements.md), including later qualifications |
| U27 | owner-settled | The CLI (agent-collaboration) and the Host are installed together, so a wire-format change ships as one cut... | [requirements.md](requirements.md), including later qualifications |
| U28 | owner-settled | One stateless MCP server (rmcp on axum) is the collaboration API | [requirements.md](requirements.md), including later qualifications |
| U31 | owner-settled | The Host's operator socket stays separate and unchanged | [requirements.md](requirements.md), including later qualifications |
| U29 | owner-settled | Out of scope, unchanged | [requirements.md](requirements.md), including later qualifications |
| U30 | owner-settled | Every new HTTP surface uses axum (servers) and reqwest (clients) | [requirements.md](requirements.md), including later qualifications |
| U32 | owner-settled | Central runs on Cloudflare | [requirements.md](requirements.md), including later qualifications |
| U33 | owner-settled | Replication is Turso CDC, per project | [requirements.md](requirements.md), including later qualifications |
| U34 | owner-settled | A Durable Object is each machine's connection, and it calls the other Durable Objects, to keep cost down | [requirements.md](requirements.md), including later qualifications |
| U35 | owner-settled | Durable Object hibernation is designed for | [requirements.md](requirements.md), including later qualifications |
| U36 | owner-settled | A logical session is separate from the physical WebSocket | [requirements.md](requirements.md), including later qualifications |
| U37 | owner-settled | A lease structure, separate from project data, fences writers (the Advisor's option B) | [requirements.md](requirements.md), including later qualifications |
| U38 | owner-settled | Leases last 4 hours and are renewed every 15 minutes | [requirements.md](requirements.md), including later qualifications |
| U39 | owner-settled | Every change of a project's holder is an explicit request, by an admin or the owner, through a separate adm... | [requirements.md](requirements.md), including later qualifications |
| U40 | owner-settled | Workstreams, not milestones | [requirements.md](requirements.md), including later qualifications |
| U41 | owner-settled | Decisions by level | [requirements.md](requirements.md), including later qualifications |
| U42 | owner-settled | The work trail ("show me your work") is part of the board, separate from the message board | [requirements.md](requirements.md), including later qualifications |
| U43 | owner-settled | Task and subtask shape | [requirements.md](requirements.md), including later qualifications |
| U44 | owner-settled | Task defaults | [requirements.md](requirements.md), including later qualifications |
| U45 | owner-settled | Holder unreachable after 30 minutes | [requirements.md](requirements.md), including later qualifications |
| U46 | owner-settled | Project metadata stays small | [requirements.md](requirements.md), including later qualifications |
| U47 | owner-settled | The work-trail row is simple | [requirements.md](requirements.md), including later qualifications |
| U48 | owner-settled | A workspace ADR's text lives in its workspace record, as Markdown; a repository file may mirror it and link... | [requirements.md](requirements.md), including later qualifications |
| U49 | owner-settled | One logical session per machine, separate from its WebSocket connection (U36) | [requirements.md](requirements.md), including later qualifications |
| U50 | owner-settled | Central is tested both locally and on Cloudflare, on the Workers Paid plan | [requirements.md](requirements.md), including later qualifications |
| U51 | owner-settled | Owner words are relayed by agents for now | [requirements.md](requirements.md), including later qualifications |
| U52 | superseded | ~~The owner holds the admin credential~~ (superseded by U57) in 1Password and the macOS Keychain, and creat... | [requirements.md](requirements.md), including later qualifications |
| U53 | owner-settled | This lane builds the lease registry and admin endpoint, with proof that it works | [requirements.md](requirements.md), including later qualifications |
| U54 | owner-settled | The upgrade keeps all history, as a separate one-time script | [requirements.md](requirements.md), including later qualifications |
| U55 | owner-settled | Central isn't exposed to the web | [requirements.md](requirements.md), including later qualifications |
| U56 | owner-settled | The projects a machine holds are separate from the projects it can message | [requirements.md](requirements.md), including later qualifications |
| U57 | owner-settled | Admin is the owner's own sign-in | [requirements.md](requirements.md), including later qualifications |
| U58 | owner-settled | Central stays on Cloudflare, behind Access | [requirements.md](requirements.md), including later qualifications |
| U59 | owner-settled | Public and private repos are split | [requirements.md](requirements.md), including later qualifications |
| U60 | owner-settled | One release | [requirements.md](requirements.md), including later qualifications |
| U61 | owner-settled | Thread references may name their project | [requirements.md](requirements.md), including later qualifications |
| U62 | owner-settled | IDs, not names | [requirements.md](requirements.md), including later qualifications |
| U63 | owner-settled | Messages and notifications reach every agent on every machine | [requirements.md](requirements.md), including later qualifications |
| U64 | owner-settled | Roles | [requirements.md](requirements.md), including later qualifications |
| U65 | owner-settled | Every thread names its subject | [requirements.md](requirements.md), including later qualifications |
| U66 | owner-settled | Wakes are bounded; schedules are for anything long-lived | [requirements.md](requirements.md), including later qualifications |
| U67 | owner-settled | One database per project stays, with one central copy per project, made manageable | [requirements.md](requirements.md), including later qualifications |
| U68 | owner-settled | Central's objects call each other only along a typed, ranked DAG enforced at compile time | [requirements.md](requirements.md), including later qualifications |
| U69 | owner-settled | One central core, injected into every runtime | [requirements.md](requirements.md), including later qualifications |
| U70 | owner-settled | SQLx is central's one database API, with swappable drivers | [requirements.md](requirements.md), including later qualifications |
| U71 | owner-settled | Cloudflare compatibility follows workers-rs | [requirements.md](requirements.md), including later qualifications |
| U72 | owner-settled | No unsafe | [requirements.md](requirements.md), including later qualifications |
| U73 | owner-settled | One lint and standards table for all our code and every fork | [requirements.md](requirements.md), including later qualifications |
| U74 | owner-settled | No pedantic tests | [requirements.md](requirements.md), including later qualifications |
| U75 | owner-settled | The Tokio runtime is a real deployment option | [requirements.md](requirements.md), including later qualifications |
| U76 | owner-settled | Exact integers everywhere | [requirements.md](requirements.md), including later qualifications |
| U77 | owner-settled | The session doesn't depend on its transport | [requirements.md](requirements.md), including later qualifications |
| U78 | owner-settled | Replication is a generic, reusable system | [requirements.md](requirements.md), including later qualifications |
| U79 | owner-settled | The board gets a new schema, built for agent-fleet-sync | [requirements.md](requirements.md), including later qualifications |
| U80 | owner-settled | Local-first doesn't mean offline | [requirements.md](requirements.md), including later qualifications |

## Selection and preservation

The read-only transfer commit is `361b7e253`; its exported requirements blob is `c2072d35`. Only selected content is applied. Neither the transfer nor the original board-design branch is merged.

| Material | Disposition |
| --- | --- |
| requirements.md, specs 1–3, PD1–PD4 and verification-design.md | Carry U38/U80 and dependent contract/binding/proof corrections, interaction ownership and the current remote-wake status. Keep unrelated newer main text. |
| Central one-core program-design.md | Remove only the three obsolete FL-8/E22 tail-report rows required by U80; all other content stays unchanged. |
| requirements-full.html | Generate an exact embed of current requirements.md, retaining the source wrapper for regeneration. Source-relative links follow the supported embed contract. |
| Design index and entity-model.md | Same as main; retained unchanged. |
| activity-record-brief.md and open-questions.md | Keep main's generic references. Transfer changes restore private-working-note links or machine labels; none is imported. |
| New board-state-spec.md, system-explained.md, system-explained-stepthrough.html, verification-design.html and what-we-learned.md | Historical or separate reader artifacts, excluded. They are unnecessary for U38/U80 and can restore superseded model/proof claims or unresolved links. |
| Additional transfer PD refinements: descriptions, receipt paging/bounds, shared-reader mechanics, cancel wiring, import attempts, registry cleanup and proof-ladder plans | Lead realizations or historical plan material, not adopted as new implementation claims here. Current main content and existing open gaps remain. |
| TUI, quota-dialog, crate DAG and every other main document outside this selection | Byte-identical to the baseline, checked by publication-manifest.json. |

[publication-manifest.json](publication-manifest.json) pins the main baseline and every protected document's Git blob. It is an integrity manifest, not a work trace. Public-safety results, negative controls and milestone observations stay in private task evidence and the Lead's work thread.
