# Test organization (2026-11-05)

Shared test code gets one owning crate or feature; no test reaches into another crate's source; production never depends on test support; rules for agents and checks that enforce them. Phase 1 moves only support code, so no test case changes identity.

- [requirements.md](requirements.md): who is affected, U1–U5, owner questions Q1 (path check) and Q2 (system-suite pilot).
- [specification.md](specification.md): the 38 cross-crate includes measured today, entities E1–E5, obligations R1–R6.
- [program-design.md](program-design.md): options A/B/C (B selected), the support units, the rules for agents, the checks.
- [verification-design.md](verification-design.md): how each need is proven, including before/after build measurements.

Reading path for the owner: requirements → program-design "Rules for agents" → verification-design "Proven by".
