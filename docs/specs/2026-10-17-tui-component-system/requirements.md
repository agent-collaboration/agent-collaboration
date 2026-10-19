# Sessions and Quota: a consistent component system

The owner needs Sessions and Quota to feel like one product: readable lists, predictable forms, searchable choices and short dialogs that use space deliberately. The current screenshots show oversized frames, inconsistent action placement, clipped account names and misaligned row metadata. A shared frame exists, but several callers bypass it and its size limits behave like fixed sizes.

This document records authorized needs. [Specification](specification.md) owns observable behavior; [Program Design](program-design.md) owns the structure. The [feedback checklist](../../wip/2026-10-17-sessions-dialog-system/owner-feedback.md) retains the conversation's actionable details and completion evidence.

## Authorized boundary

Start from origin/main `ffced1c1c3c84e0ca41701fb9eb4326acea2451e` on `codex/sessions-ux-redesign`. Cover the common TUI components and every menu, filter, editor, confirmation and Details surface reachable from Sessions and Quota. Preserve capabilities, identity and effect semantics. Quota is included by the owner's later correction. The owner likes the current HTML visual direction and explicitly chose **one New session form, then Start**.

Activity/Following, board storage, Host migration and production process replacement are outside this work. The later owner correction adds Claude background launch and real Claude/Cursor Sessions functionality; this explicitly supersedes the initial provider-preservation-only limit for those actions. The owner subsequently approved the shared Router catalogue for native background jobs: both Claude-owned and Router-managed sessions remain supported, including existing interactive Claude sessions, while new Claude launches through agent-sessions use background mode. This admits the necessary public inventory projection and source-local read adapter, not a new session store, provider lifecycle or Host migration. Default delivery is a reviewed, tested PR ready for review, unmerged. Existing CLI/static output remains functional; this is not a storage or transport migration.

## Needs and outcomes

All rows are required for this delivery; none is a discretionary enhancement. The assigner is the owner in the current conversation, with the repository constraints below.

| ID | Owner need and source | Desired outcome |
| --- | --- | --- |
| U1 | “make new branch form main origin”; “dont remove any features unles explcily asked” | Isolated work from current main, with a preservation inventory and no silent deletion. |
| U2 | “i wanted a stand component system”; “all menus and filters should use the dialog system” | One shared implementation for frame, header, body, actions and interaction, used by every Sessions/Quota menu. |
| U3 | “dont nest dialogs for choices”; “one level is good” | One visible dialog level; a choice expands inside its current form. Help or workflow transitions replace content and preserve return state. |
| U4 | Dialog can grow by x%/y% or max clamp; later correction: “i want it to be clamped and % for height and width” | Both dialog axes default to independently clamped percentages, bounded by actual space. Content sizing and explicit full screen remain supported options. |
| U5 | Remove footer/shortcuts; subdued Help in title; yellow selection; intentional hierarchy | One cyan title, white ordinary content, grey secondary/help, yellow focus/selection; predictable action placement and no persistent app footer. |
| U6 | “fields align”; selected/unselected indentation was already fixed and must not regress | Shared field columns and a permanently reserved selector gutter. Sessions title/provider/metadata align; Quota keeps its agreed three-column groups stable under selection. |
| U7 | Machine choice with search; “same for providers”; any choice field | Reusable searchable selection with understandable names, preserved selected identity and consistent keyboard/mouse behavior. |
| U8 | Explicit answer: “Show one form, then Start” | New session shows machine and directory before Start; no New Details → separate machine page sequence. Supplied arguments remain. |
| U9 | “dont remove folder scope filters”; preserve all features | Folder scopes, provider/source/runtime filters, sorting, text/full-ID search and paste remain reachable. |
| U10 | Enter opens Details; Resume/Fork/Copy ID in Details; Ctrl+Enter/modified Fork shortcut | Readable existing-session Details and eligible actions retain source identity, history and existing shortcut behavior. |
| U11 | Quota names visible, provider below name, concise labels/glyphs; “same things for quota”; annotated header asks for glyphs/icons; latest design continuation preserves current column grouping | Keep Account / Status-and-counts / Quota groups. Improve name-column allocation and wrapping, provider beneath name and compact understandable labels. Original clients/resets/windows/credits/pace/reasons remain in their groups. Wide headers teach glyph meanings with short labels; narrow headers retain glyph/count, spaces and dot separators. Reset credits use ↻. |
| U12 | “why62k has so many decimal places” | Compact readable balances such as62.5k; readable precise Details; stored/source values unchanged. |
| U13 | Explicit design/implementation/Lead skills, “make sure you have a verifier”, “use workers and sidekick” | Lead-owned design/acceptance, paired independent verification, meaningful tests, real compiled screenshots and current review/CI. |
| U14 | “make todolist”; “dont forget any feedback” | One complete, current feedback list with honest states and linked evidence; HTML is a proposal, never app proof. |
| U15 | “all claude agents shokdu be backgorudn agents”; literal `agent-sessions --provider claude --new` | Native Claude starts and resumed/forked agents use Claude background mode; CLI returns the actual background identity/outcome. The TUI uses the same launch contract. Existing hosted ACP agents remain managed background sessions. |
| U16 | “make sure claude and cursor works with agent-session tui”; remote/multiple providers and UI work together | Real supported client actions work through agent-sessions on the selected machine, preserving endpoint-qualified IDs, directory, trust and uncertain-outcome guards. Read-only rows and mock-only proof cannot close this requirement. |

## Preservation is part of the outcome

Sessions retains Cwd/Checkout/Repo/Any CLI semantics, provider/client filters, Interactive/Subagents/All request semantics, runtime views, updated/created sorting, full UUID search and normal paste, source refresh, machine loading/error/cancel states, session Details, native New/Resume/Fork/Copy ID and eligibility, original arguments, history and wide detail inspection. The current picker only exposes three root variants and no source-changing control: those implementation limitations cannot authorize removing the external capabilities.

Quota retains provider and disabled-account filters, all account information, focused/sidecar Details, contextual Help, account-options Resets/Credits tabs, credit policy editing/refresh, weekly-floor editing, reset inspection/review/confirmation/result and inventory paging. The prior proposed two-summary-row cap is superseded; the owner wants the existing information presented clearly.

## Constraints

- Use the existing iocraft system and semantic palette; layout is nested Views, gaps, flex and real text children, never manually padded formatted strings.
- Preserve existing domain/effect owners and uncertain-outcome protections. No new storage, networking, credentials, second runtime, retry policy or provider capability is authorized.
- Actual compiled TUI observation must cover Sessions and Quota, wide120×36, compact76×30, narrow48×24 and resizing/constrained-height cases. Invented-data HTML and reconstructed pictures are not runtime proof.
- Follow repository test/quality/release requirements. No proof deletion or weakening. Do not replace production processes.
- Owner attention is for product/structural tradeoffs; assignment, sequencing, bounded fixtures and cleanup are Lead responsibilities.

## Coverage boundary

Accepted requirements set: U1–U16 above, including every named preservation variant and constraint. No owner meaning is currently missing for the described cleanup. Structural feasibility, independent design review, implementation and runtime proof are separate work; this document does not claim any of them complete.
