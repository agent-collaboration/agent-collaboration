# Delivery overview: the MCP facade comes first

The first PR makes existing ordinary MCP tools usable by the actual operator and proves the session lifecycle through the real Host and each agent’s qualified capabilities. It preserves the unchanged Codex CLI. **No ACP tunnel is delivered or planned.**

The [canonical first-deliverable plan](2026-11-05-mcp-facade-proof-boundaries.html) owns the implementation scope and proof. This page is its reading map. Shravan merges PRs; the first PR cannot claim every later facade or ACP feature complete.

## First deliverable: operator control and a proven lifecycle

| Step | Result | Detailed plan | Completion check |
| --- | --- | --- | --- |
| F0 — Admit the base | Current source, real caller, provider access and shared writers are ready. | [Source and target admission](2026-11-05-mcp-facade-proof-boundaries.html#mcp-facade-lifecycle-plan--f0-admit-current-source-and-the-real-target) | Checked prerequisites, truthful caller/Approver and bounded debug target. |
| F1 — Operator control | The Human operator can prompt/cancel without borrowing a session; existing callers keep working. | [Actor boundary and repair](2026-11-05-mcp-facade-proof-boundaries.html#mcp-facade-lifecycle-plan--f1-carry-the-actual-operator-identity) | Permanent old-build failure then real MCP/application/Host-controlled success; native Human refusal before dispatch. |
| F2 — Repeatable conformance | A lasting TypeScript MCP lifecycle driver and complete coverage inventory. | [Driver and causal proof](2026-11-05-mcp-facade-proof-boundaries.html#mcp-facade-lifecycle-plan--f2-build-the-lasting-conformance-journey) | Actual Host/runtime chain, cursor/output/control cases and wrong-build sensitivity. |
| F3 — Actual agents and CLI | Claude and Cursor complete real MCP turns and unchanged CLI turns on the frozen tuple. | [Provider and CLI proof](2026-11-05-mcp-facade-proof-boundaries.html#mcp-facade-lifecycle-plan--f3-real-claude-and-cursor-then-unchanged-cli) | Exact session/turn/operation IDs, actual output/terminal/readback, original wterm frames and owned cleanup. |
| F4 — One reviewable PR | A coherent first facade change with honest capability coverage. | [Review and PR](2026-11-05-mcp-facade-proof-boundaries.html#mcp-facade-lifecycle-plan--f4-independent-review-and-the-pr) | Required checks/CI, independent review, comments/threads/mergeability; stop before merge. |

F0 → F1 → F2 → F3 → F4 is the required delivery order. Independent driver authoring can overlap only after its consumed contract is frozen and shared files have one writer. Real provider runs do not overlap product edits.

## What the operator gets

Create or load a Claude/Cursor conversation through normal MCP tools, prompt it, observe its real output, inspect its operation, continue the same conversation through its qualified native resume or load/reopen path. Native close is used where the backend actually offers it. Cancelling a waiter detaches that waiter; explicit cancellation targets admitted work and retains its actual outcome.

The current application already supports Human requesters, but public prompt/cancel inputs narrow them to SessionRef. The first repair carries the existing identity through without adding authentication or changing permissions. Reported requester and content-author fields remain distinct; no new equality guard breaks established callers.

## What proves this result

A scripted provider backend or successful tool catalogue is insufficient. Controlled integration keeps the production MCP/application/Host/runtime and substitutes only the external agent. Actual qualification then uses Claude and Cursor separately, with real inference replies and same-session readback. The unchanged CLI must complete real turns over the existing provider sockets. All runs use declared source/artifacts, caller, effects, budget, deadline and cleanup.

[Open the deciding observations](../verification-design.html#verification-design--f-ordinary-mcp-facade-qualification) · [Open the complete facade design](../mcp-facade.html).

## Later deliverables remain visible

| Delivery after the first PR | Where its obligations live | What the first PR does not claim |
| --- | --- | --- |
| Remaining facade representations | [MCP capability inventory](../mcp-facade.html#mcp-facade--capability-inventory-and-discovery) | Rich input, arbitrary config categories, client MCP setup and extension/callback mappings are not all qualified by the text lifecycle. Each gap keeps its owning design/proof before implementation. |
| Provider-bound standard incoming ACP | [Earlier bound-ACP delivery input](2026-11-04-acp-interfaces-session-tools.html) | The separate socket, metadata-free routing, settings/close and actual client-tool setup proof remain unexecuted I1–I5 scope. |
| Earlier full CLI compatibility | [CLI roadmap](../delivery-plan.html) and [command inventory](../../2026-10-31-codex-cli-acp-actions/command-inventory.html) | The legacy G2–G4 outcomes are not declared solved by this first PR. |
| Hierarchical tasks | [Application feature boundary](../mcp-facade.html) | Task result ownership is separate from messages and provider turns; no task engine/storage is delivered here. |

This is sequencing, not a waiver. No tunnel stream sits behind these deliverables. A current admitted plan is required for each subsequent implementation scope; the predecessor's plan-only context is not automatically executable.

## Dependencies and forecast

Recheck current Fleet file ownership before shared MCP/client/Host changes: adopt landed storage D1 or obtain an explicit bounded handoff. The held access F4a lane later adopts B's actual published setup shape. Do not wait for every future Fleet feature or overwrite an older branch's view of later source.

After F0, the first plan estimates 3–5 active working days: F1 0.5–1, F2 1–1.5, F3 1–1.5 and F4 0.5–1. These are initial estimates, not measured durations or calendar commitments; provider/account/owner waits and CI queue time are excluded. Re-estimate after F1's integrated result.

[System design](../index.html) · [MCP facade](../mcp-facade.html) · [Canonical first PR plan](2026-11-05-mcp-facade-proof-boundaries.html).
