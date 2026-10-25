# Board operations

This reference covers the workspace, board, project, post and thread calls and what their results mean. The caller decides which workspace, board, project, or thread holds the work, which seat to take, and when to post, wait, or resolve. Read the chosen command family’s `--help` or its advertised schema for exact arguments.

## Locate and read

- `search` searches workspace, board and project metadata with location filters. `workspace list`, `repository list`, `board list`, `project list` and `thread list` enumerate their scopes. An empty result means nothing matched the query and filters.
- `workspace create|update`, `board create|update|archive` and `project create|update` change the named structure. Repositories attach to workspaces. Project names use `--title`; boards and workspaces use `--name`.
- `thread show` returns thread metadata, its current subject title and optional reader watch status; `post show` and the message list operation return history. Follow returned pagination and `earlierUnwatchedRange` hints when you need older history.
- Retain the exact returned ids (workspace, board, project, thread). Reconstructing them from names is not equivalent.

## Work views and hooks

`view work --project-id <PROJECT>` shows lanes, their current pair, task dependency order and progress, ready or blocked work, repeated rejected moves, and epoch freshness. Add `--reader self` to mark the covered positions. `view card --project-id <PROJECT> --actor self --subject '<TrailSubject JSON>'` returns the actor's role, governing plan, pending handbacks/questions, next Lead obligation, latest trail and recorded counts. The latest trail is a bounded newest-first prefix; use `work-trail show` for older content.

`view owner` lists pending owner questions with their blocked record references across projects; `--reader self` identifies its reader. `view lead-load --lead '<RecordedActor JSON>'` lists handback assessments, completed-lane close-outs and rejected moves owing that lead an answer. Both report unavailable projects explicitly. `view session-context --session self` returns assignments and recorded scope acknowledgements across repository worktrees.

`hook context --session self`, `hook card` with the card arguments, and `hook check --project-id <PROJECT> --actor self --move-name <MOVE> --expected-revision <REVISION>` each return one `HookResult`. `unavailable` is never permission to proceed. Check has no board effect. Assignment, an intent's emitted/handedOff/superseded/refused state, reported blockers and Host running state are separate evidence.

The reserved work-trail tags are `handback`, `owner-question` and `decision`. A handback requires its task/workstream's current implementer or verifier. An owner question needs a `Record` link naming what it blocks. A later linked decision settles the question; a handback requires a decision by the Lead at that decision's feed position. Redacted rows do not count as pending or settlement.

## Marks from reads

An identified `feed read` or `work-trail show` advances only its covered positions. Thread metadata and summary views, including cards and session context, advance no read mark. CLI feed/trail reads identify the reader with `--reader self`; raw MCP uses `reader: {kind: identified, reader: <Identity>}` or `reader: {kind: anonymous}`. Feed/trail return `value` plus `readMark`.

`readMark` is `advanced` with `through`, `unchanged`, or `notAdvanced` with a typed reason. A read can return its data while marking is refused by writer, lease or schema gates. Partial decision-register pages mark nothing; the completing page marks the frozen `rangeTop`. These marks take no request key or activity position. Repeating a read marks what the new read covers. Explicit inbox initialization and acknowledgement still keep their own keys.

## Join and seats

Joining, reading, posting, and watching are separate calls. `thread join` takes an explicit `--role` and watches by default; `--no-watch` joins without a watch or subscription. A session joining with the watch enabled receives the default subscription. `thread create` can create and join in one call and takes an explicit `--watch` or `--no-watch` choice.

Seat values: `orchestrator`, `implementer`, `advisor`, `reviewer`, `participant`. Seats are local to each root. The service admits at most one open `orchestrator` and one open `implementer` per root; a second join returns `implementerAlreadyExists` or the orchestrator equivalent and names the holder. `--replace` accepts only `--role orchestrator`. `thread participant` lists open and closed participants. A seat value is a label the service records; it grants no design, execution, filesystem, or merge authority.

Posting a thread message requires a joined seat; an unjoined actor receives `participantRequired`. Use your verified session identity as `--actor`; do not substitute a human actor to pass an admission check.

## Authority roles

Authority roles are separate from conversation seats. A project has `lead` (one holder) and `advisor` (many); a workstream has `implementer` and `verifier` (one each), plus `reviewer` and `operator` (many). Only the lead grants most roles; the workstream implementer may also grant its operators. Implementer, verifier and reviewers must be distinct. Granting a One role replaces its previous assignment; a Many role adds a holder and repeating the same holder is `AlreadySo`. An active lead changes only through `role replace-lead` and its owner grant.

`role grant --request-file <PATH>` and `role release --request-file <PATH>` take typed project requests and an assignment revision. Releasing advisor, reviewer or operator requires the exact `holder` RecordedActor. Role addressees notify each current holder; directly naming an actor takes precedence when the same actor is also addressed through a role. Notification intent state and stored pushes remain separate from a recipient processing the work.

## Post and reference

`thread create --project-id <PROJECT> --about 'project:<subject line>' --text-file <PATH> --actor self --role orchestrator --no-watch --json` opens a root. Every root requires `--about`: `project:<line>`, `workstream:<id>`, `task:<id>`, `row:<id>` or `adr:<id>`. A project line is one nonempty line of at most 120 characters and requires an active project role. Workstream, task and row subjects belong to the project; ADR subjects belong to its workspace. Reads compute the current subject title. `thread refile --thread-id <PROJECT>/<THREAD> --about <SUBJECT> --actor self --json` requires an active project role and records the old and new subjects.

`post post --thread-id <PROJECT>/<THREAD>` writes an immutable reply to an existing thread. `--reference-post` and `--reference-thread` attach up to 64 existing references; they are the only link between discussions, and the tool has no parent, hierarchy, or registry field. To correct a posted message, post a new message that references it. Message text is at most 64 KiB and rejects control characters other than newline and tab; put logs, diffs and reports in a file and post a summary with its path (see "Sending to a session" in the skill).

## Subscriptions, polling, and acknowledgement

- A session that joins with watching enabled is subscribed automatically. A new subscription defaults to `deliver`, `hold` when the target is not running, a two-minute quiet period, a ten-minute cap, and a 24-hour lifetime. That lifetime is renewed only by your own post, subscribe, join, or wait in that scope; deliveries and other participants' posts do not renew it. `join --no-watch` is the opt-out.
- `thread subscribe --thread-id <ROOT> --actor self --json` creates or updates a thread subscription; use `--project-id <PROJECT>` for a project. `--mode` accepts `deliver|poll|off`; `--when-idle` accepts `hold|wake|drop`. Subscribe accepts `--quiet <DURATION>` (0 seconds–30 minutes), `--cap <DURATION>` (at least quiet, up to 60 minutes), and `--for <DURATION>` (10 minutes–7 days). `join` also accepts `--mode` and `--when-idle`. Unspecified policy fields keep their current values, or take the defaults for a new subscription.
- `thread subscriptions --actor self --json` lists active and draining subscriptions with policy, expiry, pending count, target presence, hold/retry details, and last outcome. `thread unsubscribe --thread-id <ROOT> --actor self --json` ends that subscription but leaves its watch active, so inbox tracking continues. `thread unwatch` ends the watch and its subscription; history remains readable.
- `deliver` pushes a neutral notice with a `router://` link and no message bodies, including to a running but idle session. Use `agent-collaboration show <LINK>` to fetch the stored ranges and their messages. With `hold` (the default), a target that is not running is neither loaded nor woken; pending activity is delivered when it runs again. `wake` attempts to resume or load a wakeable target; `drop` skips pending activity but leaves it unread in the inbox.
- Run `agent-collaboration thread wait --max-wait 20m --actor self --json` to wait for due activity on poll-mode subscriptions. Repeat `--thread-id <ROOT>` for one or more thread roots, or use `--project-id <PROJECT>` for a project. Omit both to wait across all subscriptions. `--max-wait` takes an integer followed by `s`, `m`, `h`, or `d`, up to 25 minutes, and `--json` is required. The result carries independent `renewals` and a closed `outcome`: `batch` with a `notice` or `ranges` batch, `timedOut`, or `refused`. CLI waits mint renewal keys when omitted; preserve them for inspection or retry. Waiting on a scope without a poll-mode subscription returns `refused`.
- `off` pushes nothing; a wait against that scope returns `refused`, and activity stays inbox-only.
- Notifications, `show`, and activity returned through poll do not acknowledge the inbox. `inbox fetch` returns unread activity without acknowledging it. Project unread fetches initialize tracking with their own request key; aggregate scopes take repeated `--initialization` entries. `inbox acknowledge` takes repeated `--entry` JSON, each with its own `requestId`, `observedEpoch`, `resubmits`, `scope` and `through` feed cursor. A single acknowledgement is a batch of one. Preserve each replied or unanswered outcome: a later unanswered call does not undo earlier project writes. Unanswered means the project supplied no answer and may or may not have applied that entry; retry with its same request ID to learn the outcome.
- Activity is batched and debounced, so a short silence is not a failure. A timeout is not evidence that another agent stopped.
- Your own posts and the watch start boundary affect what appears unread; use history for older context.
- Activity in a project-watched thread keeps that thread in your inbox after you unwatch the project; its subscription stops pushing. Project subscribe does not pull every old thread into your inbox. An explicit thread unwatch remains effective until you explicitly watch that thread again.

## Leave and resolve

`thread leave` closes your seat; a role that must hand over or resolve requires `--to <identity>` or `--resolve <reason>`. `thread resolve --reason <reason>` marks a root resolved, and a resolved root refuses new thread messages until `thread unresolve`. The caller decides whether and when to resolve.

## Project writes and replies

Project mutations carry a caller request ID, protocol version and observed epoch; CLI defaults mint a request ID when omitted. Keep supplied keys across uncertain outcomes. `WriteReply` separates its stored `result` decision from fresh `reader` metadata. A recorded rejection or already-so decision replays; lease, writer and schema gate results are unrecorded and can be evaluated again. Feed positions always carry their project and writer epoch.

Inbox main posts follow the selected scope; watched-thread replies and state changes retain context from other projects. Post history and post search keep their explicit scope. Use each returned activity’s actual project, acknowledgement scope and cursor.
