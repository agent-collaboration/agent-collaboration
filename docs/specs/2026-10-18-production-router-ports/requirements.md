# Production Router ports

A normal Host must use the owner's selected ports without requiring repeated command-line overrides. The current MCP default still selects 8788; the production proxy already selects 19741. The required normal pair is proxy **19741** and loopback MCP **19742**. Existing private Tailscale HTTPS exposure uses **8443**.

- **P1:** Make 19742 the omitted MCP bind for a normal production Host; preserve proxy 19741.
- **P2:** Preserve isolated-debug defaults 18787/18788, explicit overrides and loopback-only MCP binding.
- **P3:** Keep active guidance and the releasable package version consistent. The owner activates staged configuration manually; shipping or installing bytes must not replace a production process.

This is a small source change from main, separate from the unfinished Sessions/Quota redesign. It adds no server, network mode, policy or configuration migration. Historical records and intentional test fixtures retain their original ports.
