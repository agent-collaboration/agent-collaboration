# Central storage reset

Human entry: [decisions.html](decisions.html) (all decisions) and [d1.html](d1.html) (deliverable D1).

- Requirements: [requirements/index.md](requirements/index.md)
- Specification: [specification/index.md](specification/index.md)
- Program Design: [program-design/d1-central-layout-and-mail.md](program-design/d1-central-layout-and-mail.md) (D1; later deliverables get their own chunk when they start)
- Verification Design: [verification-design/d1-central-layout-and-mail.md](verification-design/d1-central-layout-and-mail.md) (D1)
- Plans: `plans/` (D1 plan after the owner approves D1's tables)
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

## Through-lines for remote-sessions, local-storage, automations

Requirements only so far ([U10](requirements/remote-sessions.md), [U11–U13](requirements/local-storage.md), [U14–U17](requirements/automations.md)); the other documents are written when D2, D3 and D4a start.
