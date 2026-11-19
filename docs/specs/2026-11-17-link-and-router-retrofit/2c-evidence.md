# 2c evidence: central delivers direct messages to Routers over the link

Evidence for Lead A. Nothing here is a decision. Anything suggested is marked
**Proposal (not decided)**. Code citations are repo-relative `path:line` on
branch `feat/central-link-pr3`. "Owner record, line N" is the 2026-11-12
workspace-storage owner decision record. "Design 2" is the 2026-11-06 central
execution model set. "Engine design" is the 2026-11-15 central F1 engine set
(deliverable 1).

Note on numbering: the record's 4a/4b split (owner record, line 300) was
reversed by the owner: "pr3 is both 4a and 4b, they are not separate
deliverables" (owner record, line 342). "2c / 4b" below means the
central-to-Router half of PR 3.

## 1. The owed-reply state machine as merged

### Where it lives

- Reducer: `crates/collaboration-central-core/src/engine/machines/owed_reply.rs`
  (220 lines), registered as `pub(crate) mod owed_reply` in
  `crates/collaboration-central-core/src/engine/machines/mod.rs:11`.
- Machine kind string: `"owed_reply"` (`owed_reply.rs:121`).
- Module doc: "One reply a Router is owed: sent over a live connection with
  window room, settled by that connection's answer, reset when the Router
  reconnects, and dropped when retention ends. Traffic drives it; it never arms
  the alarm." (`owed_reply.rs:2-4`).

### States (`owed_reply.rs:31-36`)

| State | Data | Terminal (`owed_reply.rs:46-51`) |
|---|---|---|
| `Waiting` | none | no |
| `InFlight` | `send_id: Uuid`, `connection: Uuid` | no |
| `Done` | `finish: Finish` (`Delivered` or `Refused`, `owed_reply.rs:19-22`) | yes |
| `Expired` | none | yes |

### Events (`owed_reply.rs:68-83`)

- `Send { connection }`: doc says "The Router is connected on `connection` and
  its window has room." (`owed_reply.rs:69-72`).
- `Answer { send_id, connection, answer: RouterAnswer }` (`owed_reply.rs:73-77`).
  `RouterAnswer` is `Accepted | RefusedForGood` (`owed_reply.rs:56-59`). There is
  no "try again" answer and no refusal reason.
- `Reconnected { connection }`: "`connection` is the new connection's id"
  (`owed_reply.rs:78-81`).
- `RetentionEnded` (`owed_reply.rs:82`).

### Effects (`owed_reply.rs:96-99`)

- `SendItem { send_id, connection }`
- `Finish(Finish)`

### Refusals

- Domain refusal: `OwedReplyRefusal::AlreadyInFlight` (`owed_reply.rs:102-104`),
  tag `already_in_flight` (`owed_reply.rs:108`).
- Everything else that is not an edge returns `TransitionError::Stale`.

### Every transition (`owed_reply.rs:131-179`)

| From | Event | Result | Line |
|---|---|---|---|
| `Waiting` | `Send { connection }` | `InFlight { send_id: context.ids.next_id(), connection }`, effect `SendItem` | 132-144 |
| `InFlight` | `Answer` with matching `send_id` and `connection` | `Done { finish }`, effect `Finish(finish)`; `Accepted` maps to `Delivered`, `RefusedForGood` to `Refused` (`owed_reply.rs:113-118`) | 145-161 |
| `InFlight` | `Answer` with a different `send_id` or `connection` | `Err(Stale)` | 156-158 |
| `InFlight` | `Reconnected { .. }` | `Waiting`, no effects | 162 |
| `InFlight` | `Send { .. }` | `Err(Domain(AlreadyInFlight))` | 163-165 |
| `Waiting` or `InFlight` | `RetentionEnded` | `Expired`, no effects | 166-168 |
| `Waiting` | `Answer` or `Reconnected` | `Err(Stale)` | 169-171 |
| `Done` or `Expired` | any event | `Err(Stale)` | 172-178 |

The declared edge table (`owed_reply.rs:181-214`) lists exactly five edges:
waiting→in_flight (send), in_flight→done (answer, terminal),
in_flight→waiting (reconnected), waiting→expired and in_flight→expired
(retention_ended, terminal). `next_due` is `None` for every state
(`owed_reply.rs:215-219`): the machine is never due and never arms the alarm.

Observations:

- `Reconnected` ignores its `connection` field (`owed_reply.rs:162` matches
  `{ .. }`). The new connection is only recorded on the next `Send`.
- `Send` while `Waiting` takes the connection from the caller. The machine does
  not check whether the window has room or whether the connection is current;
  that is the caller's job.
- There is no resend while in flight on the same connection. This matches owner
  record, line 299 ("No resend on the Router's own traffic and no timer"). It
  differs from design 2's diagram, which has an
  `in_flight --> in_flight: next_attempt_at passed ... resend, backoff` edge
  (design 2, owed-work.html line 111).

### About "window"

The word appears only in doc comments (`owed_reply.rs:2`, `owed_reply.rs:69`).
There is no window count, size or check in the reducer, its store or its probe
entry. The engine design's specification says "waiting → in flight on a send
while the Router is connected with window room" (engine design,
specification.html line 116) and its program design labels the edge "Send
(Router connected, window has room)" (engine design, program-design.html line
153). No window size is fixed anywhere in merged code.

### Its store: `engine/owed_reply_store.rs`

- Compiled only with the `test-support` feature
  (`crates/collaboration-central-core/src/engine/mod.rs:19-20`).
- Module doc: "Test-support store adapter ... over the probe tables (P10). Rows
  carry machine_kind='owed_reply'; the production table and link wiring belong
  to deliverable 4." (`owed_reply_store.rs:1-3`).
- Reads and writes **only** `engine_probe_items`, filtered by
  `machine_kind='owed_reply'` (load `owed_reply_store.rs:28`, save
  `owed_reply_store.rs:131`, finish `owed_reply_store.rs:149`, failure guard
  `owed_reply_store.rs:26`), plus a counter on `engine_probe_groups` for each
  `SendItem` (`owed_reply_store.rs:143-146`).
- Stored state strings: `waiting`, `in_flight`, `delivered`, `refused`,
  `expired` (`owed_reply_store.rs:31-43`). `Done` is split by finish.
- `send_id` and `connection_id` are written only for `InFlight`, NULL otherwise
  (`owed_reply_store.rs:114-122`). Decode demands UUIDv7 for them
  (`owed_reply_store.rs:50-56`, `72-75`).
- `Finish` sets `completed_claim=claim`; a unique index on `completed_claim`
  makes a second finish impossible (`owed_reply_store.rs:147-151`;
  `crates/collaboration-central-core/tests/fixtures/engine_migrations/0001_engine_probe.sql:22`).
- The probe table exists only in a test fixture migration: "Test-support only.
  The host's fixture migrator appends this after production migrations; no
  production database receives these machine subjects."
  (`crates/collaboration-central-core/tests/fixtures/engine_migrations/0001_engine_probe.sql:1-2`,
  table at `:9-21`, columns include `send_id TEXT` and `connection_id TEXT` at
  `:17-18`).

### How it is reached

- `crates/collaboration-central-core/src/lib.rs:10-11`: `pub mod engine_owed_probe`
  behind `#[cfg(feature = "test-support")]`.
- `crates/collaboration-central-core/src/engine_owed_probe.rs:1-2`: "Test-support
  entry ... No production route or table reaches it."
- `seed_owed_reply` inserts a `waiting` probe row (`engine_owed_probe.rs:30-54`,
  insert at `:49`).
- `request_owed_reply` admits the caller as a **workspace caller** (agent
  session or person), resolves an actor, and runs
  `Engine::request_engine::<OwedReply, _>` with `OwedReplyStore`
  (`engine_owed_probe.rs:56-90`). It is shaped like a workspace request, not a
  link frame from a machine.
- Cloudflare test routes `/test/engine-owed-seed` and
  `/test/engine-owed-request` call these
  (`crates/collaboration-central-cloudflare/src/workspace_cases.rs:196-214`
  region; file doc at `:1` says test-support bundles only).
- No production code sends `Event::Send`, `Answer` or `Reconnected`.

### Tests

Pure, in `crates/collaboration-central-core/src/engine/machines/owed_reply_test.rs`:

- `every_owed_reply_state_event_pair_matches_the_declared_edges` (`:84`): every
  state × event pair against the declared edges; `next_due` is `None` for all.
- `owed_reply_each_arrow_produces_its_exact_effect` (`:107`).
- `owed_reply_refuses_late_answers_double_sends_and_finished_work` (`:172`).
- `owed_reply_same_inputs_time_and_ids_give_the_same_result` (`:229`).
- Hegel property test
  `owed_reply_sequences_leave_refusals_unchanged_and_finish_once` (`:247-248`):
  random sequences of up to 50 events over two connections; asserts at most one
  finish and that finished work stays finished.

Through `handle` on both hosts (V1, P10):

- `crates/collaboration-central-tokio/tests/integration/workspace_requests/mail/engine_owed_reply.rs:69`
  `a_late_answer_to_an_older_send_after_reconnect_changes_nothing`, `:169`
  `two_answers_racing_on_one_owed_reply_commit_once`.
- The same two tests on workerd:
  `crates/collaboration-central-cloudflare/tests/integration/workspace_requests/mail/engine_owed_reply.rs:69`, `:169`.

## 2. The wire frames today

### Frames (`crates/central-link-protocol/src/link_codec.rs`)

```rust
// link_codec.rs:45-50
Deliver {
    message: Box<MessageRecord>,
},
Ack {
    message_id: MessageId,
},
```

- Direction: `Ack` is Router→central, `Deliver` is central→Router
  (`link_codec.rs:69-77`).
- `Welcome` carries only `machine_id` and `central_time` (`link_codec.rs:31-34`).
  No connection id goes on the wire.

### `MessageRecord` (`crates/workspace-model/src/central_mail.rs:214-225`)

```rust
pub struct MessageRecord {
    pub message_id: MessageId,
    pub sender: MessageSender,      // Actor(ActorIdentityId) | Central (:209-212)
    pub target: SessionAddress,
    pub body: MessageBody,
    pub state: MessageState,        // Queued | Delivered | Cancelled | Failed { reason } (:195-200)
    pub source: Option<NoticeSource>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

`MessageKind` is `Direct | Notice | Wake | Automation` (`central_mail.rs:186-191`).

### What uses them today

- Router client: `Deliver` becomes `ClientEvent::Deliver { message }`
  (`crates/central-link-client/src/link_state.rs:260-262`); an incoming `Ack` is
  `UnexpectedFrame` (`link_state.rs:277-279`). Nothing on the Router builds an
  `Ack`.
- Central, Cloudflare: `Deliver` and `Ack` arriving at central close the socket
  with "unsupported link event"
  (`crates/collaboration-central-cloudflare/src/workspace_machine_link.rs:95-103`).
- Central, Tokio: the fallthrough arm closes with `close_code::UNSUPPORTED`,
  "unsupported link operation"
  (`crates/collaboration-central-tokio/src/workspace_link.rs:255-265`). A test
  pins this: `acknowledgements_are_explicitly_unsupported_and_shutdown_drains_open_links`
  (`crates/collaboration-central-tokio/tests/integration/workspace_link.rs:236-253`).
- Central never sends `Deliver` today: no central host constructs it (only
  `workspace_machine_link.rs:100` and the codec mention it).

### Why these frames cannot settle the owed-reply machine

1. **No `send_id`.** `Event::Answer` needs the `send_id` minted on `Send`
   (`owed_reply.rs:73-77`, `133`), and an answer whose `send_id` differs is
   `Stale` (`owed_reply.rs:156-158`). `Deliver` does not carry a send id to the
   Router, so the Router cannot echo one, and `Ack { message_id }` has none.
   Matching by `message_id` alone cannot tell a late answer to an older send
   from the answer to the current send, which is the case the machine and its
   tests exist for (`a_late_answer_to_an_older_send_after_reconnect_changes_nothing`).
2. **No connection identity.** `Event::Answer` and `Event::Send` need a
   `connection: Uuid`. No such id exists today: the hibernation attachment is
   organization, machine, installation, enrollment, client and expiry only
   (`crates/collaboration-central-core/src/machine_connection.rs:17-24`), and
   `ValidatedMachineConnection` adds only the installation machine id
   (`machine_connection.rs:54-57`). `client_id` is per enrollment, not per
   socket. A per-socket id would have to be minted at upgrade and stored in the
   attachment (on Cloudflare it must survive hibernation). The Router does not
   need to send it: central knows which socket a frame arrived on.
3. **No answer kind.** `Ack` cannot say `RefusedForGood`, which the machine
   accepts (`owed_reply.rs:56-59`).
4. **One frame per message.** There is no batched form, so acks cannot ride
   the Router's minute flush as one frame.

### Proposal (not decided): frame shapes

```rust
// Proposal (not decided). central-link-protocol DataFrame additions/replacements.

/// Central to Router: one or more owed messages on this connection.
/// Each item has a fresh UUIDv7 send_id minted by the owed-reply Send edge.
Deliver {
    items: Vec<DeliverItem>,           // or a single { send_id, message } if no batching
},

/// Router to central: answers for items received on THIS connection.
/// Central takes `connection` from the socket the frame arrived on.
Ack {
    acks: Vec<DeliveryAck>,            // rides the Router's 1-minute flush (owner record, line 218)
},

pub struct DeliverItem {
    pub send_id: SendId,               // UUIDv7, echoed in the ack
    pub message: Box<MessageRecord>,   // message_id inside is the inbox de-dup key (owner record, line 300)
}

pub struct DeliveryAck {
    pub send_id: SendId,
    pub message_id: MessageId,         // redundant with send_id; lets the Router and central cross-check
    pub answer: DeliveryAnswer,
}

pub enum DeliveryAnswer {
    Accepted,                          // in the recipient Router's inbox
    RefusedForGood { reason: DeliveryRefusal }, // typed reason; the reducer's RouterAnswer has none today
}
```

Proposal (not decided), the unbatched variant if batching is not wanted:

```rust
Deliver { send_id: SendId, message: Box<MessageRecord> },
Ack { send_id: SendId, message_id: MessageId, answer: DeliveryAnswer },
```

Proposal (not decided): `Welcome` gains `connection_id: ConnectionId` so the
Router can log which connection a delivery belonged to. Not needed for
matching.

Points the proposal touches but does not settle:

- `DeliveryRefusal` reasons do not exist yet; `RouterAnswer` would need a
  reason if the sender is told why (owner record, line 207: "outcomes and
  problems only").
- `MessageRecord.state` is sent to the Router as `Queued` today
  (`crates/central-link-client/src/link_liveness_test.rs:126`); it carries
  central's view, not the delivery's.
- A batched `Ack` is one frame inside the minute flush. Whether it rides inside
  `DataFrame::Batch` (max 64 writes, owner record, line 199) or as its own frame
  is open.

## 3. Storage

### Current `messages` table

Exact SQL, `crates/collaboration-central-core/migrations/central/0002_mail_notices_timers.sql:3-6`:

```sql
CREATE TABLE messages (message_id TEXT PRIMARY KEY, sender TEXT NOT NULL, target_machine_id TEXT NOT NULL REFERENCES machines(installation_id), target_session TEXT NOT NULL, message_kind TEXT NOT NULL, body TEXT NOT NULL, state TEXT NOT NULL, failure_reason TEXT, source_change_id TEXT, recipient_actor TEXT, source_workspace_id TEXT, source_thread_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX messages_queued_by_machine ON messages(target_machine_id, message_id) WHERE state = 'queued';
CREATE UNIQUE INDEX messages_one_per_source ON messages(source_change_id, recipient_actor) WHERE source_change_id IS NOT NULL;
CREATE INDEX messages_queued_by_thread ON messages(source_workspace_id, source_thread_id, recipient_actor) WHERE state = 'queued';
```

No later migration in `migrations/central/` (0001, 0003, 0004) touches
`messages`.

### How delivery state is stored today

- One `state` column with `queued | delivered | cancelled | failed`, plus
  `failure_reason` only for `failed`. Encoded in
  `crates/collaboration-central-core/src/central_mail_rows.rs:32-37`, decoded in
  `crates/collaboration-central-core/src/central_mail_records.rs:63-74` (any
  other combination is `corrupt("messages.state")`). Validation is in Rust; no
  CHECK.
- Insert: `central_mail_rows.rs:39-40`.
- No production SQL sets `state='delivered'` today (search of
  `collaboration-central-core/src` finds the encoder/decoder only). There is no
  send id, connection or in-flight marker.
- `queued_for_session` (`central_mail_records.rs:118-134`) reads
  `WHERE target_machine_id=? AND state='queued' ORDER BY message_id`, filters to
  the session in Rust (`:128`) and does not consume rows. It is used by tests
  (`crates/collaboration-central-tokio/tests/integration/workspace_requests/mail/timing_contract.rs:287`
  and others).
- Notice rows in the same table are rewritten by notice windows
  (`crates/collaboration-central-core/src/notification_windows.rs:209`, `:219`).
- The merged `central_work_attempts` table is failure-only (comment at
  `crates/collaboration-central-core/migrations/central/0004_central_work_attempts.sql:1`,
  table at `:2-16`): no `send_id`, no `connection_id`.

### Repository and owner rules any option must meet

- Every table has a UUIDv7 primary key and `created_at` / `updated_at`
  (AGENTS.md, "Identifiers").
- SQL CHECK only for booleans 0/1; enum membership validated in Rust and on
  decode (AGENTS.md, "SQLite And Rust Validation").
- Undeployed central migrations are edited in place (owner record, line 332:
  "Decision 2 = A, edit central migrations 0001/0002 in place"). With an in-place
  edit there is no SQLite table rebuild; any store that ran the old file is
  reset.
- Table names carry a concern prefix (owner record, line 233: "all talbes
  hsoudl ahve namespace"); `messages` itself is listed there as violating it.
- R5 (owner record, line 245): "one central_work_attempts table ... **existing
  tables gain no attempt columns**". Whether `send_id`/`connection_id` count as
  "attempt columns" is not recorded.
- The 15-minute rule (section 4, decided 2026-11-17): whichever option is
  chosen, the stored shape must support cancelling at 15 minutes. That needs an
  accepted-at time to measure from and a cancelled end state for the delivery.
  Today `messages` has `created_at` and a `cancelled` state value
  (`central_mail_rows.rs:35`); the owed-reply reducer has neither a cancel
  event nor a cancelled state (section 1). How central notices the 15-minute
  mark without arming the alarm for messages (owner record, line 278) is open.

### Proposal (not decided) A: carry owed-delivery state on `messages`

```sql
-- Proposal (not decided). In-place edit of 0002 (or ALTER TABLE ADD COLUMN in a new file).
CREATE TABLE messages (
    message_id TEXT PRIMARY KEY,
    -- ... existing columns unchanged ...
    state TEXT NOT NULL,              -- adds 'in_flight' as a Rust-validated value, or keep 'queued'
    delivery_send_id TEXT,            -- UUIDv7, set only while in flight
    delivery_connection_id TEXT,      -- UUIDv7 of the socket the send went out on
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX messages_delivery_send ON messages(delivery_send_id) WHERE delivery_send_id IS NOT NULL;
CREATE INDEX messages_in_flight_by_connection ON messages(delivery_connection_id) WHERE delivery_send_id IS NOT NULL;
```

Costs:

- No new table. Two nullable columns; `ALTER TABLE ... ADD COLUMN` of a nullable
  column with no default does not rebuild the table in SQLite, and the in-place
  edit avoids even that.
- Existing rows: none in production (record line 332); other rows read NULL.
- Readers: the owed-reply store, `decode_messages`
  (`central_mail_records.rs:29-113`), `queued_for_session`, notice windows. If
  `in_flight` is added to `state`, `MessageState` (a wire type,
  `central_mail.rs:195-200`) and the `messages_queued_by_machine` partial index
  change meaning; keeping `state='queued'` while in flight avoids that but splits
  the machine's state across two columns.
- One row mixes the message's content with its delivery attempt. Notices,
  wakes and automations share the table (`message_kind`), so the columns would
  be NULL or reused for those kinds.
- Tension with R5's "existing tables gain no attempt columns" (owner record,
  line 245).
- 15-minute cancel: `created_at` could serve as the accepted-at time and
  `state='cancelled'` as the end state, both already present; the reducer
  would need a cancelled ending that maps to it.

### Proposal (not decided) B: a new table for owed deliveries

```sql
-- Proposal (not decided). New migration, or in-place addition to 0002.
CREATE TABLE central_owed_deliveries (
    owed_delivery_id TEXT PRIMARY KEY,                       -- UUIDv7
    message_id TEXT NOT NULL REFERENCES messages(message_id),
    target_machine_id TEXT NOT NULL REFERENCES machines(installation_id),
    state TEXT NOT NULL,                                     -- waiting | in_flight | delivered | refused | expired (Rust-validated)
    send_id TEXT,                                            -- UUIDv7, only while in_flight
    connection_id TEXT,                                      -- UUIDv7, only while in_flight
    refusal_reason TEXT,                                     -- only when refused, if a reason is carried
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX central_owed_deliveries_message ON central_owed_deliveries(message_id);
CREATE UNIQUE INDEX central_owed_deliveries_send ON central_owed_deliveries(send_id) WHERE send_id IS NOT NULL;
CREATE INDEX central_owed_deliveries_waiting ON central_owed_deliveries(target_machine_id, owed_delivery_id) WHERE state = 'waiting';
CREATE INDEX central_owed_deliveries_in_flight ON central_owed_deliveries(connection_id) WHERE state = 'in_flight';
```

Costs:

- New table, no change to `messages`; no rebuild of anything.
- Columns line up one-to-one with the reducer state and with the probe
  table's `send_id`/`connection_id` (`0001_engine_probe.sql:17-18`), so the
  owed-reply store moves from the probe table with little change.
- Each queued direct message needs a second row written in the same batch as
  the message insert; reads of "what is owed to machine X" join or read this
  table. `messages.state` would still need to move to `delivered` on `Finish`
  (or be read from here), so the two must stay consistent.
- The primary key is a fresh UUIDv7, with `message_id` a UNIQUE constraint, per
  AGENTS.md "any other key becomes a UNIQUE constraint". Using `message_id` as
  the primary key would also satisfy the UUIDv7 rule ("A key that is already
  UUIDv7 stays the key").
- 15-minute cancel: the table would need its own `accepted_at TEXT NOT NULL`
  (or read `messages.created_at`) and a `cancelled` value in `state`
  (Rust-validated, no CHECK), plus an index on the waiting/in-flight rows by
  accepted-at if central scans for overdue rows.

### Recorded option C (from the design docs, not proposed here)

- Design 2 puts owed rows in `central_work_attempts` with
  `send_id, connection_id, session_id, sent_version` (design 2,
  program-design.html line 183) and counts the window from it (line 224).
- The engine design says of `central_work_attempts`: "This deliverable builds
  the failure-only slice; deliverable 4 extends it for owed sends with its own
  migration." (engine design, program-design.html line 174).
- The merged table is failure-only (`0004_central_work_attempts.sql:1`) and
  keyed one row per `(machine_kind, subject_key)` (`:17-18`).

## 4. Owner-record lines that constrain 2c

Quoted exactly, typos kept.

| Line | Exact words | What it constrains |
|---|---|---|
| 9 | **all agent messages** ("all messages go through central") | Every direct message passes through central. |
| 60 | "**Unknown native result:** never acked, so central keeps it queued and the Router hands it over again ... Delivery to the Mac is at least once." | Earlier (11-12) delivery guarantee: at least once; duplicates possible. |
| 112 | **R9:** "if acp acks, why? just trust and move on". | The session's acknowledgement counts as delivered (Router-to-agent side). |
| 142 | "lets set this up to batch but set the batching to every one minute for now" ... "ok all messages 1 minute for now. approvals immediate" | One-minute batching; approvals immediate. Superseded in part by 169. |
| 165 | "Unchanged: direct messages batch every whole minute (may move to a 5-minute multiple later), approvals are immediate" | Batch interval may change later. |
| 168-169 | "right roguht batches is good … b soudns better make smroe sense"; "**Central delivers a direct message at once**, during the wake its send request already caused, with no minute alarm ... **The recipient's Router batches** each agent's messages per minute ... An offline recipient's messages stay queued in central until its Router reconnects." | Central sends at once to a connected recipient; no central timer; offline means queued until reconnect. |
| 170 | "Storage: `messages.deliver_after` is **dropped**" | No deliver-after column. |
| 197 | "yes i think flushign is a inportant quice of this" | Router outbox flushed on the minute mark. |
| 201 | "the design has no message expiry/give-up rule (an ended session's message is re-offered forever) — open owner decision" | `RetentionEnded` has no decided trigger. |
| 202 | "the receitps back to sesison shoduk lbe batch edcby sesiosn and be token efficient" | Receipts batched per session. |
| 205 | "StatusStream to sender; 60-day retention, expiry, caps, cancels, Uncertain" (Lead, listing dropped owner requirements) | Prior owner delivery requirements still being recovered. |
| 207 | "Pushed statuses: (a) outcomes and problems only; and "it shoudl able to pull the status with a request if it wants to in teh api of the mcp"" | Sender gets outcomes/problems pushed; any message's status can be pulled on request. So delivery state must be readable per message. |
| 218 | "**DECIDED: acks ride the recipient Mac's minute flush (a)**, plus a new owner requirement: "if ntohign tcan be delvierd in 15min in central and it fails we need to let teh agent know the soruce agent". Exact meaning (a 15-minute bound at central vs. the 15-minute notice slot) is OPEN" | Acks are sent on the recipient's 1-minute flush, not at once. The 15-minute part is now settled (next row). |
| (not yet in the record) | "15 minutes you can't send in central or machine then tell the sender and cancel send" (owner, relayed by Lead A, 2026-11-17) | **Decided.** Closes the OPEN meaning at line 218. See Lead A's reading below. |
| 225 | "default deadline comes from Router settings.jsonc and travels on each message as valid_until ... The 15-minute rule reads as either a deadline or a warning. Both open." (Lead proposal, NOT decided) | Only a proposal; no `valid_until` exists. |
| 233 | "all talbes hsoudl ahve namespace" | New table names carry a prefix. |
| 245 | "then we only have to maintain one table and its failures or repeated failures to retire and stop them from making things stuck" ... "**existing tables gain no attempt columns**" | Failures go in `central_work_attempts`; caution on adding columns to `messages`. |
| 276 | "ok so messages are sent in 1 min cadence" | Messages on a 1-minute cadence (Lead reading of central vs Router side still to confirm). |
| 278 | "both events get sent in 1 minute marks and we process based on the schedule. not alarms, a schedule in durable object … central is not operating every minute you misunderstood that" ... "messages never arm the alarm" ... "Still open: how a failed delivery to a connected machine is retried." | Routers send messages, acks and status on minute marks; central handles each on arrival, including delivery; messages never arm the alarm. |
| 280 | "jitter i was thinking +-3s" | Router flush jitter ±3 s. |
| 285 | "owed work, batches and send_ids go with deliverable 4, the link (wire changes)" (Lead's split) | send_ids and batches are link work. |
| 294 | "**the reply is a state machine.** "is the resposte meta stae wshousl it be a state machien with our state system?" then "yes kpelse ukpdate teh design to for engien reply statemchine … it amke sit estable and fvalaidatable"" ... "waiting → in_flight (send, new UUIDv7 send_id) → done (answer accepted)" ... "2. yes all events are answer immediately." | The owed reply is a reducer with a fresh UUIDv7 send_id per send; events are answered immediately. |
| 297 | "**"no, it goes to b's inbox".** A Router never answers central "send it again later": every valid message goes into the recipient Router's own inbox ... The owed-reply machine has only Accepted and a final refusal; the only resend is after reconnect." | No "try again" answer; acceptance means "in the recipient Router's inbox". |
| 299 | "**"a is fine, lets move on".** Central's next try to send to a Router is only that Router's reconnect (Welcome). No resend on the Router's own traffic and no timer; a message whose answer was lost on a live socket waits for the reconnect." | Only reconnect resends; a lost ack on a live socket waits. |
| 300 | "4b must include inbox de-duplication by message ID so a reconnect resend never doubles a message." (Lead) | Router inbox de-dups by `message_id`. |
| 342 | "pr3 is both 4a and 4b, they are not separate deliverables" ... "central→Router (deliver at once if connected, Welcome resend, inbox de-dup)" | One PR; deliver at once, Welcome resend, inbox de-dup. |

### The 15-minute rule (decided 2026-11-17)

Owner's words, relayed by Lead A on 2026-11-17: "15 minutes you can't send in
central or machine then tell the sender and cancel send". This settles the
meaning left OPEN at owner record, line 218. It is not yet written into the
owner record.

**Lead A's reading** (Lead A's, not the owner's words):

- A direct message not delivered within 15 minutes, whether still in the Mac
  outbox or waiting at central, is cancelled and the sender is told. There is no
  keep-trying.
- On the Mac: an Unsent message older than 15 minutes is cancelled and removed
  from the flush, with a "cancelled: not sent in 15 min" outcome to the sender.
  A SentAnswerLost message at 15 minutes ends "not confirmed: may have been
  delivered", because central's replay cannot be undone.
- At central: a message not delivered to the recipient within 15 minutes of
  acceptance is cancelled and the sender is told. This is part of the 2c design.
- Still OPEN with the owner: whether thread posts get the same 15 minutes or
  keep 24 hours.
- Nothing is being built for this yet.

What the merged code has against this reading:

- The owed-reply reducer has no cancel or timeout event and no cancelled
  state; its only non-delivery ending is `Expired` on `RetentionEnded`
  (`owed_reply.rs:82`, `166-168`), and it is never due (`owed_reply.rs:215-219`).
- `messages.state` already accepts `cancelled` (`central_mail_rows.rs:35`,
  `central_mail_records.rs:67`), used today for emptied notices
  (`notification_windows.rs:219`).
- `messages.created_at` exists (`0002_mail_notices_timers.sql:3`); whether it
  is the acceptance time for the 15-minute bound is not stated anywhere.

Search terms used on the record: deliver, inbox, ack, send_id, owed, window,
15min, minute, alarm, retention, flow. The record contains no line about the
flow-control window size (see section 5).

## 5. Window size options

### What design 2 says

- "Window of 32 unanswered pushes per connection" — status "My proposal, not
  yet reviewed by you" (design 2, decisions.html line 51, row R12).
- "Window of 32 unanswered pushes per connection" — "My proposal; tell me if
  you want a different number" (design 2, assumptions.html line 97, O6).
- "Window: 32 unanswered socket requests per connection (proposed), counted
  from rows each time a send is considered." (design 2, owed-work.html line 142).
- "Window: counts items, so 32 in flight per connection is at most one full
  batch." (design 2, owed-work.html line 89).
- "Limits: at most 32 items or 1 MiB per batch, whichever comes first ... An
  item bigger than 1 MiB goes alone." (design 2, owed-work.html line 88).
- Batches: owner words "yes i allow batches for all request types which is an
  ordered array of same request type"; "i meant know the order"; status column
  "Decided. ≤ 32 items or 1 MiB; one batch per sender→recipient pair in flight"
  (design 2, decisions.html line 46, R10). The quoted owner words do not name
  the numbers.
- R15: "Never have more than the window of socket requests unanswered on one
  connection ... A request waiting for a slot is never a due item" (design 2,
  specification.html line 101).
- Design 2 also orders per sender→recipient pair: "the next batch for that pair
  is sent only after the previous batch for that pair is answered" (design 2,
  owed-work.html line 94).

### What the owner record says

- No line in the owner record sets, accepts or rejects a window size. The
  "design input, not settled" wording the brief mentions was not found in the
  owner record, the engine design, the link retrofit spec folder, or PR 3's
  `tmp/` notes. The latest recorded status is design 2's "My proposal, not yet
  reviewed by you" (decisions.html line 51).
- The engine design keeps "window has room" as a condition without a number
  (engine design, specification.html line 116; program-design.html line 153).

### Options seen

| Option | Source | Recorded status |
|---|---|---|
| 32 unanswered items per connection | design 2, decisions.html line 51; owed-work.html line 142 | Lead proposal, not reviewed by the owner |
| Batch ≤ 32 items or 1 MiB | design 2, decisions.html line 46 | Batching decided by owner; numbers in the status column |
| One batch in flight per sender→recipient pair | design 2, decisions.html line 46; owed-work.html line 94 | Same as above |
| Window of 1 per message (owed-reply `AlreadyInFlight`, `owed_reply.rs:163-165`) | merged reducer | Built; it is per message, not per connection |
| **Proposal (not decided):** window of 1 per connection (strict ordering, one outstanding batch) | added here | none |
| **Proposal (not decided):** unbounded per connection (send every waiting row at once; rely on Welcome resend) | added here | none |
| **Proposal (not decided):** size-bounded only (bytes in flight, e.g. 1 MiB), no item count | added here | none |

### A fact that bears on the choice

Acks ride the recipient Mac's minute flush (owner record, line 218), and the
Router flushes on 1-minute marks ±3 s (owner record, line 280). An answer can
therefore take up to about a minute. With a per-connection window of N items, a
backlog drains at roughly N items per minute per connection. Design 2 assumed
answers came back promptly; its drain-speed risk X3 ("Drain speed of a large
backlog through a window of 32") is listed in design 2, assumptions.html line
106.

## Open questions for Lead A

1. The 15-minute cancel is decided (section 4). How does central reach the
   15-minute mark for an undelivered message, given "messages never arm the
   alarm" (owner record, line 278) and `next_due = None`
   (`owed_reply.rs:215-219`): checked on traffic, in the due batch, or by
   making the owed reply a due machine?
2. Does the 15-minute cancel replace `RetentionEnded`/`Expired` for direct
   messages, or sit beside it, given no expiry rule was decided before (owner
   record, line 201)?
3. What happens when a Router answers `Accepted` for a message central has
   already cancelled at 15 minutes (the message is in the recipient's inbox but
   the sender was told "cancelled")?
4. Open with the owner: do thread posts get the same 15 minutes, or keep
   24 hours?
5. Where does the per-socket connection id come from, and is it stored in the
   Cloudflare hibernation attachment (`machine_connection.rs:17-24`)?
6. Does `Reconnected` fire for every owed row of a machine on Welcome, or is
   "in flight on a connection that is not current" read as waiting (design 2,
   owed-work.html line 118)?
7. Is a window needed at all when the only resend is reconnect (owner record,
   line 299), and if so, is it per connection, per sender→recipient pair, or
   per message?
8. Do acks travel inside `DataFrame::Batch` or as their own frame in the same
   flush?
9. Does `RefusedForGood` carry a typed reason back to the sender, and which
   refusals exist?
10. Storage: columns on `messages`, a new prefixed table, or an extended
   `central_work_attempts`, given R5 (owner record, line 245) and the prefix rule
   (owner record, line 233)?
11. Does 2c cover only `message_kind='direct'`, or also wake, automation and
   notice rows that live in the same table?
12. What does the pull-status request (owner record, line 207) return while a
    message is in flight versus in the recipient's inbox?
13. Does the existing Tokio test that pins `Ack` as unsupported
    (`workspace_link.rs:236-253` in the Tokio integration tests) get replaced in
    the same PR, and with which proof (Hegel/Turmoil per owner record, line 355)?
