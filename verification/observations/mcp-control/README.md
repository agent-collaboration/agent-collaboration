# MCP control observations

`run.ts` is the only entry: `node verification/observations/mcp-control/run.ts PLAN.json [--check]`.
The recipe is `.agents/skills/verify-codex-router/references/mcp-control-journeys.md`.

| Folder          | Owns                                                                                   |
| --------------- | -------------------------------------------------------------------------------------- |
| `admission/`    | The plan schema and its pre-run checks, and the kept continuation record.              |
| `inventory/`    | The ACP schema coverage inventory (`acp-inventory.json`) and its loader and checks.    |
| `transport/`    | The streaming MCP client and the typed tool requests the journeys send.                |
| `journeys/`     | The lifecycle, control and continuation cases and the steps they share.                |
| `observations/` | Observation models, the ledger and turn folding, and the case oracles and settlements. |
| `evidence/`     | Owning-store role readback, the controlled peer receipt and the release sentinel.      |
| `test-support/` | The scripted Host stand-in and its plan; tests only, never imported by the driver.     |

Each test sits beside the file it tests.
