import type { Identity } from "./declaration.ts";
import type { Installation } from "./machines/installation.ts";
import type { CodexSession } from "./recipients/codex-session.ts";

export interface JourneyMachine {
  readonly installation: Installation;
  readonly recipient: CodexSession;
  readonly actor: Identity;
  readonly heldProject: string;
}
