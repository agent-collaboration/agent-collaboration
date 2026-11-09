# ACP interfaces, shared control and delivery

The owner-selected direction is an ordinary MCP facade over existing agent control, alongside incoming ACP and the unchanged Codex CLI. The first lifecycle deliverable has its own reviewed design and PR plan; it does not claim full facade/ACP/CLI completion. Tasks remain separate hierarchical work; messages retain loose delivery semantics. No tunnel is delivered or planned.

The interface design includes the owner-selected ordinary MCP facade. An ACP-message tunnel is excluded entirely. Start with [MCP agent control](mcp-facade.html), then the full system map: [start with the full system and DI map](interface-design.html), then follow the Requirements, Specification, Program Design and Verification Design below. The interface scope covers R16–R22 while preserving the earlier CLI obligations and their named open gaps.

- [Requirements](requirements.md): people, outcomes and fixed boundaries.
- [Specification](specification.md): entity meanings and observable obligations.
- [Program Design](program-design.md), including its normative [interface chunk](interface-design.md): incoming/outgoing profiles, per-session client MCP tools, DI, command/result matrix, module/service/data map, provider registration and feature extension; existing detailed native/control rules remain linked.
- [MCP facade](mcp-facade.md): the normative Program Design chunk for U13/R23, existing tools, complete capability mappings and output/control semantics. [HTML reader](mcp-facade.html).
- [Verification Design](verification-design.md): the deciding evidence, provider-effort preservation and false-pass cases.
- [Milestones and workstreams](delivery-plan.md): ordered delivery, parallel limits, completion checks and timing forecast.
- [Delivery overview by deliverable](plans/overview.md): outcomes, sequence, proof and forecasts, with a direct plan link for each deliverable. [HTML reader](plans/overview.html).
- [Interface implementation plan](plans/2026-11-05-mcp-facade-proof-boundaries.md): the current first R23/V23 lifecycle deliverable, F0–F4 and proof gates. Its terminal is PR-ready-unmerged; Shravan merges. The earlier bound-ACP plan remains future delivery input, not a completed capability. [HTML reader](plans/2026-11-05-mcp-facade-proof-boundaries.html).
- [Bounded M1 continuation plan](plans/2026-11-02-m1-provider-settings-proof.md): preserve the implemented model/default/auth contracts and valid Select journeys; earn the remaining actual provider-settings branch proof. Its [HTML reader](plans/2026-11-02-m1-provider-settings-proof.html) is the human entry. Terminal remains pr-ready-unmerged under A's repair commission; broader outcomes remain open.
- [Detailed CLI command inventory](../2026-10-31-codex-cli-acp-actions/command-inventory.html): 65 slash variants plus other UI surfaces.

## Reading path: understand the system

1. [What we are building](requirements.md)
2. [The whole system](interface-design.md#the-whole-system)
3. [Inputs, outputs and completion](interface-design.md#inputs-outputs-and-completion)
4. [Milestones at a glance](delivery-plan.md#milestones-at-a-glance)

## Through-line: incoming ACP

1. [U10 and the capability map](requirements.md#people-and-outcomes)
2. [R16/R18–R20 and C16–C20](specification.md#interface-scenarios-and-their-references)
3. [Profiles, caller binding and capability construction](interface-design.md#selected-incoming-acp-profiles)
4. [The client's MCP tools](interface-design.md#the-clients-mcp-tools-belong-to-the-requested-session)
5. [Deciding interface observations](verification-design.md#e--complete-incomingoutgoing-interface-qualification)

## Through-line: outgoing ACP and our features

1. [U11/U12](requirements.md#people-and-outcomes)
2. [R17/R21/R22](specification.md#observable-obligations)
3. [Registration and feature extension](interface-design.md#extending-the-system)
4. [Provider, feature and preservation proof](verification-design.md#e--complete-incomingoutgoing-interface-qualification)

## Concept: authority and identity

Meaning: [E1/E3/E8/E11/E12](specification.md#the-things-we-must-keep-distinct). Structure and picture: [bound profiles](interface-design.md#selected-incoming-acp-profiles) and [system view](interface-design.md#the-whole-system). Need: U8/U10; proof: Verification E. A connection, session, operation and pending approver are separate identities.

## Reading path: first useful delivery

1. [Clean model selection need](requirements.md#people-and-outcomes)
2. [Observable default-save contract](specification.md#observable-obligations)
3. [First CLI changes](program-design.md#first-cli-changes)
4. [M1 workstreams](delivery-plan.md#m1--clean-model-selection)
5. [Bounded M1 continuation plan](plans/2026-11-02-m1-provider-settings-proof.md)
6. [Actual client/provider proof](verification-design.md#b--original-client-and-actual-provider)

## Reading path: reusable ACP control

1. [Current contract differences](program-design.md#current-contracts-we-must-preserve)
2. [Incoming standard/extension map](program-design.md#incoming-acp-what-exists-and-what-does-not)
3. [Shared execution proof](verification-design.md#d--shared-execution-and-stronger-contracts)
4. [Exact cancellation ownership](program-design.md#exact-cancellation-keep-the-effects-inside-the-actor)
5. [Later bounded expansion](delivery-plan.md#f1--broader-acp-control-expansion-alongside-cli-work)

## Concept: Fleet and local agent control

Meaning: [authority/custody R13 and supplier scope R18](specification.md#observable-obligations). Structure: [Fleet compatibility boundary](interface-design.md#compatibility-with-fleet-work). Proof: [Fleet boundary preservation](verification-design.md#fleet-boundary-preservation-for-r13r18r20r22). The [milestone plan](delivery-plan.md#f1--broader-acp-control-expansion-alongside-cli-work) shows the integration order.

## Through-line: ordinary MCP agent control

1. [U13 and fixed boundaries](requirements.md#people-and-outcomes)
2. [R23 and C23/V23](specification.md#observable-obligations)
3. [Existing owners and MCP mappings](mcp-facade.md#capability-inventory-and-discovery)
4. [Output and recovery semantics](mcp-facade.md#output-observation-without-a-new-subscription-owner)
5. [Deciding MCP qualification](verification-design.md#f--ordinary-mcp-facade-qualification)
6. [Real-provider proof profiles](verification-design.md#real-provider-proof-profiles)
