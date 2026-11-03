# Program Design: D2, Fleet sends and remote create

How D2 realizes [R16–R22](../specification/remote-sessions.md) on top of D1's Fleet object and Mail protocol ([D1](d1-central-layout-and-mail.md)). Evidence: `tmp/d2-inventory/01-fleet-sends.md`, `02-remote-create.md` (Worker, source-verified at `fb3cf6b5`). Every table rule from D1 applies (UUIDv7 primary key, timestamps, typed columns).

## What you will be able to do

From a session on A: `agent-collaboration conversation create --machine <B> --endpoint codex-local --cwd /path/on/B --access workspace-write [--model …] [--effort …] [--text "first input"]`. You get a request id at once, then “B has your request”, then a message in your session from the Fleet: the new session's address, or “B said no, because …”, or “not sure”. You can message the new session like any other.

## The shape: A asks, the Fleet sends, B creates

```mermaid
flowchart LR
  S["Session on A"] -->|"request (machine socket call)"| FA["Fleet: check A and B enrolled<br/>(temporary rule)"]
  FA -->|"Fleet message: create-session"| FO["Fleet outbox"]
  FO -->|push| BI["B inbox"]
  BI --> BC["B command owner<br/>create once under the request id"]
  BC -.->|"report: created / refused / unknown"| FO
  FO -->|"Fleet message: result"| SI["A inbox → requesting session"]
```

The request is a call to the Fleet over A's socket, not mail, because only the Fleet may originate a Fleet message (R21). The command and the result are both Fleet messages on D1's mail path. The Fleet drops a command's content once B accepts it, and the requester lives only in that content, so D2 keeps two small records (below): one in the Fleet, one on B.


## Tables (for the owner before code)

| Table | Store | Columns | Why it is needed |
| --- | --- | --- | --- |
| `fleet_remote_creates` | Fleet | `id` UUIDv7 PK (= request id, minted by A before its first call and reused on every retry), `command_digest`, `requester_machine_id`, `requester_endpoint`, `requester_session_id`, `target_machine_id`, `command_item_id`, `result_item_id` (UUIDv7, chosen at admission), `created_at`, `updated_at` | the requester must survive the command's content being dropped; the same id with a different digest is refused here; the result message id is fixed up front so a retried report cannot send a second result |
| `mail_create_commands` | each machine (`interaction.sqlite`) | `id` UUIDv7 PK (= request id), `command_digest`, `create_operation_id` (= request id), `prompt_operation_id` (UUIDv7, allocated once, stored before the prompt is sent), `create_outcome` (`pending`, `created`, `refused`, `unknown`), `new_session_*`, `settings_outcome` (`all applied`, `partial`), `prompt_outcome` (`not requested`, `not attempted`, `accepted`, `failed`, `unknown`), `created_at`, `updated_at` | binds the command to the one local create and the one prompt it may cause; a replay answers from this row; an operation the command did not create is never adopted |

When the Fleet applies B's report for a create command, it admits the result message (`result_item_id`) in the **same transaction** as the report, so a crash between the two cannot lose the result.

## Components and changes

| Component | Change |
| --- | --- |
| `collaboration-mail-protocol` | `MailSender::Fleet` (no fields; the Fleet is one per organization). `MessageBody` accepts a Fleet sender. New body `CreateSession { requester: SessionRef, endpoint, working_directory, access, model?, effort?, mode?, first_input? }`; the request id is the item id |
| Delivery address (`BoardDeliveryTypes::Address`) | becomes `Session(SessionRef) \| Machine` so a command can be addressed to a machine without inventing a session. Stored serialized, as D1's consumer-value reason allows |
| Fleet request handler (central core, new) | takes the request id A minted (a retry reuses it; a different command under the same id is refused), authenticates A's socket machine, checks A and B enrolled (**temporary rule; code comment says so and links R17**), freezes the command, admits a Fleet-origin outbox item with sender `Fleet` and recipient `Machine` on B. Answers `accepted {request_id}`, `replayed {current status}` or `refused {sender not enrolled \| target not enrolled \| conflict \| too old}` |
| Fleet status query (new call) | `remote create status {request_id}` from the requesting machine: the command item's status, and its result once reported |
| Fleet completion observer (new) | on B's report for a create-session item, admits the result message under the pre-chosen `result_item_id` in the same transaction as the report, addressed from `fleet_remote_creates` |
| B command owner (Router, new) | `MailConsumer` routes a `CreateSession` body to the command owner (today's `Wake` body uses the same branch pattern, `remote_wake`). It admits a local create with **operation id = request id** through the existing owner: Codex ACP create, or the external-provider create (`conversation_operations.rs:99`, supervisor `prepare_operation`). If the operation already exists, it answers from its record; after `MayHaveDispatched` it reports `unknown` and never dispatches again |
| Creator and approver on B | today an actor is only a local session or the human owner (`provider_identity.rs:12–18`). D2 adds one identity, `RemoteRequester { session }`, accepted **only** from the command owner's trusted path (never from a caller), stored as the creator on the provider session record; the approver is B's local owner (`--approver-owner`), so the new session's tool approvals appear on B as for a local create (R22). The plan confirms the same binding for the native Codex create owner |
| First input | sent only when the create outcome is `created` with all requested settings applied; the prompt operation id is allocated once and stored in `mail_create_commands` before sending, and reused on any replay. Outcomes: `accepted`, `failed`, `unknown` (the prompt may have been delivered), `not attempted` (create pending or settings partial), `not requested` |
| Report outcome | the create result rides D1's Report as `delivered` with a typed acceptance `CreateSessionResult { created {session, settings applied, partial settings}, refused {reason}, unknown }`, the same way the wake reply rides today (`CarrierAcceptance::Wake`) |
| Recipient projection | `PushOrigin::Fleet`; push lines label the sender “Fleet” (`received_mail_push.rs`) |
| CLI and MCP | `conversation create --machine`; `conversation_create` gains `machine`; `conversation remote-status --request-id` |

## Failure cases

| Case | What happens |
| --- | --- |
| B offline | command waits in Fleet's outbox; A sees “waiting for B”; pushed after B's welcome |
| A or B not enrolled | refused at the Fleet request; nothing stored |
| Same request id, different content | refused as a conflict at the Fleet; and at B's inbox by digest |
| B receives the command twice | B's inbox recognises the id; the command owner answers from the recorded operation |
| B crashes after starting the session, before recording the result | on restart the operation is `MayHaveDispatched`; B reports `unknown`; no second session |
| Create succeeds, first input fails | result `created` with “first input not delivered” |
| A offline when the result is ready | the result message waits in Fleet's outbox for A |
| A machine submits an item with sender `Fleet` | refused at Fleet admission (R21) |
| B never takes the command | it waits within D1's 60-day retention; when the removal reaches an undelivered command, the Fleet sends the requester “expired” in the same transaction |
| Fleet applies the report, then crashes before sending the result | cannot happen: the report and the result message are admitted together |
| B crashes after the prompt is sent, before the result is reported | the stored prompt id is reused; the prompt outcome is `unknown` if B cannot tell; no second prompt |

## Trace

| R | Owner | Interface | Proof seam |
| --- | --- | --- | --- |
| R16 | Fleet request handler | request call | request and refusal |
| R17 | Fleet request handler | enrollment check | code comment and test name |
| R18 | Fleet status query, completion observer | status call, result message | stage reads and one result |
| R19 | B command owner, B inbox | operation id = request id | duplicate push; crash after start |
| R20 | B command owner | first-input prompt | first input accepted / refused |
| R21 | mail protocol, Fleet admission | `MailSender::Fleet` | sender shown at recipient; spoof refused |
| R22 | B creator/approver binding | approver = B's owner | approval appears on B |

## Not in D2

Workspace membership and permissions; Fleet messages produced by automations (D4a, D4b).
