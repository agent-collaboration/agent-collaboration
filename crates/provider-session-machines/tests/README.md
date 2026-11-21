# Provider session machines tests

This pure crate has no integration or e2e target. Every machine's tests sit beside it in its sibling `*_test.rs`:
- each machine's declared-transition check (every state × event pair reaches exactly the declared edges);
- Hegel properties over event sequences (for example, a line's sequence numbers stay unique, an admission matches a reference model of unknown outcomes, and a refusal's next steps equal the moves the reducer allows);
- hand-written semantic examples of the M8 answers.

The cells that hold these states, and the effects they run, are tested where they live: in acp-client-runtime and codex-router-host.
