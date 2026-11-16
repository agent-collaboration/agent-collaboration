import type { LedgerSection } from "../schema-audit-types.ts";

const P = "crates/agent-session-tui/src/session_picker/";

export const section: LedgerSection = {
  root: "crates/agent-session-tui",
  files: [
    {
      file: `${P}test_support/picker_r15_instance.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-session-tui/src/session_picker/test_support/mod.rs:503 (parent cfg(test) at session_picker/mod.rs:43)",
      },
      note: "R15 mount; its json! snapshot (L64) is a test journal, not a protocol",
    },
    {
      file: `${P}toolbar/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}toolbar/picker_toolbar/mod.rs`,
      disposition: { kind: "none" },
      note: "terminal UI toolbar layout and scope switch; no protocol shape",
    },
  ],
  sendSites: [],
};
