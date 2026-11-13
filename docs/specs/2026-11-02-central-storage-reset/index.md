# Central storage reset

Human entry: [decisions.html](decisions.html) (all decisions) and [d1.html](d1.html) (deliverable D1).

- Requirements: [requirements/index.md](requirements/index.md)
- Specification: [specification/index.md](specification/index.md)
- Program Design: [program-design/d1-central-layout-and-mail.md](program-design/d1-central-layout-and-mail.md) (D1; later deliverables get their own chunk when they start)
- Verification Design: [verification-design/d1-central-layout-and-mail.md](verification-design/d1-central-layout-and-mail.md) (D1)
- Plans: `plans/` (D1: `2026-11-03-d1-central-layout-and-mail-r6.md`, delivered; D2: `2026-11-06-d2-fleet-sends-and-remote-create.md`)
- Evidence (not authority): `tmp/d1-inventory/` in this worktree, Worker source inventory at `fb3cf6b5` / library `b9da4b5`

## Through-line: central-layout (D1)

1. [Requirements: central-layout](requirements/central-layout.md)
2. [Specification: central-layout](specification/central-layout.md)
3. [Program Design: stores and Fleet tables](program-design/d1-central-layout-and-mail.md#stores-after-d1)
4. [Verification Design: U1–U3](verification-design/d1-central-layout-and-mail.md#rows)

## Through-line: mail (D1)

1. [Requirements: mail](requirements/mail.md)
2. [Specification: mail](specification/mail.md)
3. [Program Design: flows and failure cases](program-design/d1-central-layout-and-mail.md#flows)
4. [Verification Design: U4, U6, U7, U9](verification-design/d1-central-layout-and-mail.md#rows)

## Reading path: B is offline when A sends

1. [U4](requirements/mail.md#u4)
2. [R7](specification/mail.md#r7), [R9](specification/mail.md#r9)
3. [Flows](program-design/d1-central-layout-and-mail.md#flows)
4. [O1](verification-design/d1-central-layout-and-mail.md#o1)

## Through-line for remote-sessions (D2)

1. [U10](requirements/remote-sessions.md#u10), with Fleet sends in [U8](requirements/mail.md#u8)
2. [R16–R22](specification/remote-sessions.md)
3. [D2 program design](program-design/d2-fleet-sends-and-remote-create.md) (r5, review closed)
4. [D2 verification design](verification-design/d2-fleet-sends-and-remote-create.md) (P1–P15, P8b, P11b)
5. [D2 plan](plans/2026-11-06-d2-fleet-sends-and-remote-create.md)

## Through-line for local-storage (D3)

1. [U7, U9](requirements/mail.md#u7) (local records), [U11–U13](requirements/local-storage.md)
2. [R23–R29](specification/local-storage.md)
3. [D3 program design](program-design/d3-local-cleanup.md) (r2, re-anchored on merged D1)
4. [D3 verification design](verification-design/d3-local-cleanup.md) (Q1–Q6)

## Through-line for automations

Requirements plus program designs ([U14–U17](requirements/automations.md) with [D4a](program-design/d4a-wakes-and-notifications.md) and [D4b](program-design/d4b-schedules.md)); their specification and verification design are written when D4a starts.
