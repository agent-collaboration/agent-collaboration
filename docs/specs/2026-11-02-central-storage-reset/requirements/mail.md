# Requirements: mail

Every message and notification goes from the sender's outbox to the recipient's inbox; Fleet routes and sends through its own outbox.

**Today.** Machines have inboxes and outboxes, but a cross-machine message reaches B through a per-machine central Mailbox that B pulls from (`crates/collaboration-service/src/collaboration_application/message_operations.rs:218`; `agent-fleet-sync` `fleet-sync-delivery/src/inbox_receive.rs:21`). Fleet cannot send: a sender is only a session, a human or a project (`crates/collaboration-mail-protocol/src/mail_content.rs:18`). Board notifications skip mail and are written straight into the local push table (`crates/collaboration-service/src/subscription_delivery/subscription_push.rs:196`). Inbox and outbox rows carry no timestamps (`fleet-sync-delivery/migrations/0001_delivery_custody.sql:17–18`).

| ID | Need | Owner's words | Authority |
| --- | --- | --- | --- |
| <a id="u4"></a>U4 | Every machine has an inbox and an outbox. Fleet has only an outbox: a message arriving at Fleet is routed straight into Fleet's outbox, which delivers it to the recipient's inbox | “you cannot send properly because what I want was never implemented: inbox outbox in all machines and fleet”; “inbox and outbox for message passing. does central need inbox why or why not? inbox to outbox is extra durable step make no sense”; yes to L0 | authorized, 2026-11-02 |
| <a id="u5"></a>U5 | Notifications are messages: they travel only through the inbox and outbox system | “notification also only go with inbox outbox system” | authorized, 2026-11-02 |
| <a id="u6"></a>U6 | Messages are sent at once. Notifications go out at most every 10 minutes, with up to 1 minute of random jitter per machine | “for now just send whenever”; “notification only every 10 minutes ok? with jitter”; “add jitter as well 1 min max per machine” | authorized, 2026-11-02 |
| <a id="u7"></a>U7 | Every message and every entity has a UUIDv7 id and timestamps. Every id we generate is UUIDv7, never UUIDv4. Ids created by Codex, Claude and other tools are theirs | “all messages and entities in my system should have uuid v7 and timestamps”; “we should never use uuidv4 i explicitly forbade it”; “only uuidv7” | authorized, 2026-11-02; `AGENTS.md` Identifiers |
| <a id="u8"></a>U8 | Fleet sends messages of its own, through the same outbox, using mail only | “enable fleet to send messages and create sessions”; “central has to send automated messages”; “fleet only uses interaction to send messages” | authorized, 2026-11-02 |
| <a id="u9"></a>U9 | Every message, on every machine and in central, is removed 60 days after it was created. A message older than 60 days is not accepted | “we will clear any outbox messages or inbox after 60 days”; “i think it should be part of this for all messages in all machines in central”; “60 days remove”; yes to L15 | authorized, 2026-11-02 |

**Also serves:** U7 and U9 also serve `local-storage`; U5 and U6 (notification cadence) also serve `automations`.

**Not asked for:** a Fleet inbox; a 5-minute outbox pickup (“actually remove that we change it later”); delivery guarantees beyond what the owner set; replicating approvals or interaction records.
