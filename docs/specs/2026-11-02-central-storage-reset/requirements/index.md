# Requirements: central storage reset

The owner's authorized needs for rebuilding central's storage, mail, remote session creation, local storage and automations. Every row quotes the owner; nothing here is derived. The human view of the decisions is [`../decisions.html`](../decisions.html).

## Who is affected

- **Agents** on any machine, which send and receive messages and notifications, and create sessions on other machines.
- **The owner**, who runs the machines, enrolls them, sets wakes and schedules, and reads mail.
- **Each machine's Router**, which holds that machine's inbox, outbox and local stores.

## Capabilities

- `central-layout`: central is one Fleet object plus one object per project, the same on Tokio and Cloudflare.
  - [U1](central-layout.md#u1), [U2](central-layout.md#u2), [U3](central-layout.md#u3)
- `mail`: every message and notification goes from the sender's outbox to the recipient's inbox; Fleet routes and sends through its own outbox.
  - [U4](mail.md#u4), [U5](mail.md#u5), [U6](mail.md#u6), [U7](mail.md#u7), [U8](mail.md#u8), [U9](mail.md#u9)
- `remote-sessions`: an enrolled machine creates a session on another enrolled machine.
  - [U10](remote-sessions.md#u10)
- `local-storage`: each Router's tables carry one prefix per concern, and JSON is kept only with a stated reason.
  - [U11](local-storage.md#u11), [U12](local-storage.md#u12), [U13](local-storage.md#u13)
- `automations`: wakes and schedules are definitions held only in central and fired by Fleet as messages; notifications are wakes.
  - [U14](automations.md#u14), [U15](automations.md#u15), [U16](automations.md#u16), [U17](automations.md#u17)

The capability list is the owner's (their keep on L1, 2026-11-02).

## Goal boundary

| Field | Boundary | Owner basis |
| --- | --- | --- |
| Goal | Agents on any machine message each other and create sessions on each other's machines; Fleet sends its own messages; automations live only in central; central is one Fleet object plus one object per project | “one org one fleet”; “enable fleet to send messages and create sessions”; “move automation to central only” |
| Existing code | Evidence of what exists, never a constraint. Each module is kept, rewritten or deleted against the model, including the delivery library `agent-fleet-sync`, which is ours and not used by AgentStudio | “the code is fucked”; “it was lazy fucking slop”; “agentstudio does not use anything related to this”; yes to L3 |
| Protected | The model-traffic proxy; the production Router and the owner's main accounts; approval and question behavior (their tables are only renamed) | `AGENTS.md`; “approvals and questions are fine this is out of scope”; L14 |
| Non-goals | Replicating approvals and interactions; workspace membership and permission rules; migrating automation data; per-firing history in central; multiple organizations; Windows | yes to L2; “out of scope for you”; “no migration at all” |
| Evidence | Real CLI and MCP journeys on a debug Router against Tokio central and staging Cloudflare central | keep on L12 |

## Delivery limits

These constrain how the work is delivered, not what it does.

| Limit | Owner basis |
| --- | --- |
| Each deliverable is one PR, one testable journey: D1 → D2 → D4a → D4b, with D3 alongside | “each of these 4 (4 might be broken up) is a deliverable”; keep on L9; “if it's another deliverable for notifications and wakes that's fine then we do automation” |
| Storage and tables are shown to the owner before code | standing rule |
| Debug Router only; nothing against production or the owner's main accounts | “no one runs against my main accounts, only debug router” |
| CI under 30 minutes | “there's a 30 minute cap on ci” |
| Coding helpers Sol 6.1; test sessions Luna; no Claude subagents | standing rule |

## Deliverables and the rows they deliver

| Deliverable | Rows |
| --- | --- |
| D1 Central layout and mail | U1, U2, U3, U4, U6 (messages), U7 (mail records), U9 (inbox and outbox) |
| D2 Fleet sends, remote create | U8, U10 |
| D3 Local cleanup | U7 (other local records), U9 (other local copies), U11, U12, U13 |
| D4a Wakes and notifications | U5, U6 (notification cadence), U14, U15, U16 |
| D4b Schedules | U14, U15, U17 |
