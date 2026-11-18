# PR 3 · Workspace contract additions (Lead A, 2026-11-17)

These are the central contract additions the Router retrofit needs. They're built in PR 3 as slice 2b: types in `workspace-model`, rules in `workspace-rules`, admission and execution on both central hosts, and tests on both hosts. The table schema doesn't change unless a row says so. Any schema change stops for Lead A.

## Sources
- The owner's record: `codex-router.feat-sqlx-do-clean/docs/specs/2026-11-12-workspace-storage/owner-decisions.md`.
- The evidence pass: `$CLAUDE_JOB_DIR/tmp/link-contract-gaps.md`.
- The CLI+MCP read list: `codex-router.feat-acp-cli-actions/tmp/f4-workspace-link/read-list.md` (34 rows, each with its exact gap).

## Additions
1. **`CreateThread` gains the first post** (`body`, `addressees`), applied in the same rule step that creates the thread and adds the creator (`workspace-rules/src/thread_rules.rs:36-63`). It's immediate. *Lead call; owner: "creation of thread should be immediate".*
2. **New writes, all immediate:**
   - `UpdateWorkspace` (name, description; your 11-14 unique-name rule applies);
   - `ArchiveWorkspace`;
   - `UpdateArea`;
   - `UpdateProject { title?, description? }`;
   - `UpdateIssue { title }`.
3. **Reads:** the 16 Show/List additions in the read list, as new `ListQuery`/`ItemRef` variants:
   - single-workspace post filters;
   - the issue project scope;
   - workspace list and show;
   - member and role listing;
   - name and description search.
   The reader comes from `caller`. Reads move no read marks; only `MarkRead` does.
4. **The `Inbox` reply** carries each item's view (title, kind, thread and area names), with the fields taken from the CLI+MCP Lead's list.
5. **`InstallationId`** (wire): UUIDv7 only. The installation is the Router's persisted peer UUIDv7, not its serviceId; a v4 Router is re-keyed at enrollment (formerInstallation, B3). Central resolves it to its `machine_id` through `machines.installation_id` (record:187). The earlier "any canonical RFC UUID" Lead call is withdrawn (owner record:353).

## Not in this slice
- **Wake and schedule operations:** they follow the S4a and S4b designs.
- **Lead replacement by a person:** waiting on the owner (`owner-pages/2026-11-17-owner-decision-stranded-lead.html`).

## Proof
- A Hegel property test per new rule: admission refuses bad input, and identical requests give identical results.
- Both-host request tests in the existing `workspace_requests` suites.
- Each new rule's match is exhaustive.

## Inbox reply fields (agreed with the CLI+MCP Lead, 2026-11-17)
Source: today's `message-board/src/board_messages.rs:294-359` at 247c13231.

**The page carries:**
- `scope`: `Area` (today's Board) or `Project`, within one workspace;
- `read_mode`: `Unread` or `Latest`;
- the ordering;
- the per-project high-water cursor;
- the list of unavailable projects;
- the records;
- the next cursor.

**Each item is one of:**
- **`PostCreated`:** position, acknowledgement scope (what `MarkRead` takes), the full post (author, body, addressees, thread, timestamps), and the thread title plus the project or area names.
- **`ThreadStateChanged`:** position, acknowledgement scope, thread, project or area, the actor, the new state, and the thread title plus names.

**Stays local:** the cross-workspace aggregate (`inbox_workspaces`).

## Input-field answers (CLI+MCP Lead's input diff, 2026-11-17; all Lead calls)
1. **`SendMessage` gains `delivery: Auto | Queue | Steer`,** carried through central into the recipient's inbox item. The recipient Router honours it when it hands the message over (its per-minute batching still applies).
2. **`ResolveThread` gains `closing_post: Option<PostText>`,** written atomically with the resolve. This is like `CreateThread`'s first post. A separate outbox post would land after the thread is resolved and be refused. No new column.
3. **`actingFor`: dropped for v1.** Attribution is the caller only. Attributing to a person comes with sign-in (`Caller::Person`).
4. **`--human-user`: out of PR 4** until the Router has sign-in for a person caller.
5. **Inbox `scope` (Area | Project) and `read_mode` (Unread | Latest):** both are carried; see "Inbox reply fields".

**Removals agreed** (hard cutover; each is refused as an unknown field): protocolVersion, the thread project qualifier, caller-chosen postId, post placement, references on posts, thread_leave's afterLeave, multi-entry acknowledgement, inbox initializations, old opaque cursors, includeRedacted, message generationGuard.
