# Observable defaults

Reference: [Requirements](requirements.md), P1–P3.

| ID | Required behavior |
| --- | --- |
| R1 | A normal Host with omitted `--mcp-bind` selects `127.0.0.1:19742`; omitted production proxy `--port` remains 19741. There is no fallback to 8787/8788. |
| R2 | Isolated-debug defaults remain 18787/18788. Explicit loopback binds, including port 0, remain unchanged. Non-loopback MCP addresses remain rejected. |
| R3 | Restart retains the captured explicit invocation. Changing a default does not rewrite or restart a running Host, apply staged files or migrate live configuration. |
| R4 | Active MCP setup guidance uses 19742 for the normal backend and describes the existing optional private Tailscale exposure accurately. Advance the workspace package to the next unused release version and update Cargo.lock before merge; preserve historical records and explicit fixtures. |
