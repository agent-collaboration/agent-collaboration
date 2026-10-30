# TUI component system (`agent-collaboration-tui`)

A reusable Rust component crate on iocraft that makes keys and clicks go to exactly one place, defines each action once, draws menus, dropdowns and dialogs cleanly on top, and gives the session picker and the quota screen the same building blocks. It builds on the accepted [2026-10-17 Sessions and Quota design](../2026-10-17-tui-component-system/requirements.md) and the [2026-10-23 quota dialog visuals](../2026-10-23-quota-dialog-visuals/); everything there stays unless the owner changed it in their own words.

| Document | Answers |
| --- | --- |
| [Requirements](requirements.md) | Why, for whom, within what boundary; U1–U15; what changes from 10-17 and why |
| [Specification](specification.md) | What must be observably true: entities E1–E8, rules R1–R16, constraints C1–C5, proof obligations V1–V16 |
| [Program Design](program-design.md) | How the crate is built: components, interfaces, state, lifecycle, failure handling, reuse and migration |
| [Verification Design](verification-design.md) | How each requirement will be shown to hold: the lasting test system and the one-time owner checks |

**Reading paths.** Owner: Requirements (goal boundary, "Relationship to the 2026-10-17 design"), then the Verification Design's "Proven by" table. Implementer: Specification R1–R16, then Program Design components, lifecycle and trace table. Verifier: Verification Design, then the Specification's V rows.

**Core.** The new components are the menu, the combo box (select) and the standard dialog. The rest are existing `codex-router-tui` pieces, moved and restyled. Underneath them sits the one owner of keys and clicks.

**Decided.** Both screens put their actions in a ≡ menu and toolbar buttons on top (owner, option A). **Open.** Physical Ctrl+Enter in the owner's Ghostty (H2) is unproven until the owner check.

**Status.** Written; independent review pending. Nothing here has been built or observed.
