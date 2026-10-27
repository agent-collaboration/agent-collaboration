# Quota and session visual refinement

## Authorized scope

The owner approved the final visual direction in the 2026-10-22/23 conversation and explicitly chose “Implement visual changes now; removal separately.” Real account removal may later be a CLI command; it is outside this delivery. The approved visual reference is [the accepted HTML study](../../wip/2026-10-22-dialog-design/index.html). This is the accepted snapshot carried into the implementation worktree.

This is a presentation refinement over the existing Sessions/Quota component system. Existing session/provider, quota-selection, credential, reset, credit-policy and effect semantics remain authoritative. No stored format, network protocol, Host lifecycle or account-lifecycle change is authorized. No production process replacement, merge, release or install is included.

The older [component-system Requirements](../2026-10-17-tui-component-system/requirements.md) remain the preservation reference. This owner's newer visual decisions supersede its exact old three-column quota grouping, old heading colors, old action/footer placement, and placement of routine pool/freshness census and problem counts in the browse header where inconsistent. Credential and operational failures remain visible in the header. Its provider/action functionality is preserved, not reimplemented here.

The HTML is adopted for composition, grouping, typography, color and spacing. Its sample action sets, letter shortcuts, Account actions/Disable/Enable/Remove, session More submenu and disclosures are not adopted. Existing supported actions and input semantics remain the authority; their shortcut labels must describe actual bindings. The new read-only Pool details view must be keyboard reachable as well as pointer reachable.

## Capability map

| Capability | Owner's purpose | Home needs |
| --- | --- | --- |
| Quota screen | See account, quota left, pace and extra balances without clutter | U1–U5 |
| Filters and dialogs | Recognize labels, focus and actions in one composed system | U6–U8 |

The direct user is a person inspecting accounts and existing sessions in a terminal; the maintainer composes existing components rather than owning another widget framework. The screenshots show wide whitespace, repeated labels, unclear headings and weak action visibility. The accepted reference demonstrates their desired replacement; HTML is not proof of compiled behavior.

All rows are owner-authorized and required for this visual delivery. Priority assigner: owner; priority: required.

| U | Authorized outcome and conversational basis |
| --- | --- |
| U1 | Quota uses Account / Quota / Pace / Extra. Restore the earlier compact Account block with name, provider plus clients, and one status. Credits and resets belong in Extra. |
| U2 | Weekly/5h share one column as `bar % · week/5h · time until reset`; no repeated “Resets”. Weekly is white; 5h is slightly softer cool white. Unknown data is explicit. |
| U3 | Clear column headers, subtle alternating account backgrounds and stronger selected state. Color does not replace meaningful labels. |
| U4 | Layout adapts to width. Folded groups use less internal spacing; whole accounts have more separation. Final approval: no blank row between folded groups, two blank rows between accounts. |
| U5 | Header is less busy; pool-wide census and freshness diagnostics stay inspectable through labeled Pool details. Avoid duplicated held/reserve/refresh reasons in browse. Credit quantity precedes “credits.” |
| U6 | Search shows an input position/caret and nested choices under a clear field heading; draft/apply/cancel behavior is preserved. |
| U7 | Session details distinguish title, labels, values, original long text and secondary metadata. Full original information remains available; bottom actions are visually distinct and remain reachable while content scrolls. |
| U8 | Use a clean shared component system and composition, as TUI-faithful as possible: one font size, cell layout, glyph borders, solid colors. Preserve existing account/session actions, shortcuts, identity, pending/error behavior and static/JSON semantics. |

## Scope and evidence limits

Permitted homes are the shared TUI crate and existing quota/session presentation/projection owners plus their associated tests and verification apparatus. Extend existing boundaries, not proxy/provider/storage ownership. No new persistence, retry loop or external service is needed. Routine pool diagnostics may move to Details; credential/operational failures must not be hidden. Full names, identities and quota facts stay inspectable when a narrow list cannot fit all of them.

Compiled real-path proof on macOS, using the existing declared terminal-grid proxies, is required for the affected UI. The previously accepted phone boundary is narrow/short desktop terminals, not physical iPhone hardware. Public provider effects are unchanged and no model call is required to establish these visual outcomes. Independent review and fitting quality gates are part of delivery.

The accepted HTML plus its actual iocraft library captures are UI design views. They establish the intended composition, not production integration. Browser file-URL inspection was unavailable to the author; owner screenshots and explicit visual corrections supply the visual reference. No fresh browser preview is inferred from owner approval.

Continue with [Specification](specification.md), [Program Design](program-design.md), and [Verification Design](verification-design.md).
