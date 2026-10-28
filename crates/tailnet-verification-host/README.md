# Tailnet verification Host

This standalone verification tool owns one isolated Host lifetime, native profile projection, process identity checks, and cleanup. It belongs to the verification apparatus and is not a product or release executable.

Build and run from the repository root, using the declared Host plan and allocation described in `.agents/skills/verify-codex-router/references/tailnet-mcp.md`:

```sh
cargo build -p codex-router-tailnet-verification-host
target/debug/codex-router-tailnet-verification-host host "$HOST_PLAN"
```

The TypeScript tailnet peer/readback driver has its own entry point. Its checks do not require this executable.

The Host keeps `O_NOFOLLOW`, nonblocking `flock`, owner/mode checks, inode/device checks, byte guards, and exact profile restoration in Rust. It signals only processes whose retained identity still matches a fresh observation. An unavailable cleanup inventory remains unknown and cannot count as empty.

Run its focused checks with:

```sh
cargo test -p codex-router-tailnet-verification-host
cargo clippy -p codex-router-tailnet-verification-host --all-targets -- -D warnings
```

Tests use disposable profiles, owned harmless children, and local TCP/Unix listeners. Captured Python inputs and results cover the original profile and observation contracts. A real two-machine Tailnet journey still requires its own declared targets and allocation; these tests qualify the apparatus.
