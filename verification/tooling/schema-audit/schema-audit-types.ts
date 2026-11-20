/**
 * Shared types of the D0 schema audit (Program Design Y4 and Y5): the code inventory, the
 * source-coverage ledger and the problem list. Data files import these; the report reads them.
 */

/** Who owns the contract a definition or message belongs to (Program Design "Who owns each contract"). */
export type ContractOwner =
  | "codex"
  | "acp"
  | "claude-extra"
  | "router-extension"
  | "cursor-extra"
  | "mcp"
  | "router-vocabulary";

export type DefinitionKind =
  | "rust-type"
  | "zod"
  | "hand-json"
  | "method-table"
  | "content-parser"
  | "session-ref-conversion"
  | "approval-table"
  | "history-renderer"
  | "json-in-string"
  | "schema-mechanism";

/** One encoding at its boundary is legitimate; a second copy is a duplicate. */
export type Disposition = "boundary-encoding" | "duplicate" | "malformed";

/** The Router parts of the protocol-contracts graph. */
export type Part =
  | "N1"
  | "N2"
  | "N3"
  | "N4"
  | "N5"
  | "N6"
  | "N7"
  | "N8"
  | "N9"
  | "N10"
  | "N11"
  | "N12";

export type Owner = { part: Part } | { crate: string };

export type FixedBy = "D1" | "D2" | "cli-project" | { external: string };

interface InventoryRowFields {
  file: string;
  line: number;
  owner: ContractOwner;
  kind: DefinitionKind;
  defines: string;
  usedBy: Owner[];
  duplicateGroup: string | null;
}

/**
 * One definition or conversion site. A boundary encoding is the one legitimate encoding at its
 * boundary: it has no fixer and never yields a finding. A duplicate or malformed site is a defect
 * someone fixes. A retired row records where a removed definition lived and the removing commit.
 */
export type InventoryRow =
  | (InventoryRowFields & { disposition: "boundary-encoding" })
  | (InventoryRowFields & { disposition: "duplicate" | "malformed"; fixedBy: FixedBy })
  | (InventoryRowFields & { disposition: "retired"; removedIn: string });

/** What one source file does with message shapes. */
export type FileDisposition =
  | { kind: "defines"; owners: ContractOwner[] }
  | { kind: "converts"; owners: ContractOwner[] }
  | { kind: "sends"; owners: ContractOwner[] }
  | { kind: "reads"; owners: ContractOwner[] }
  | { kind: "none" }
  | { kind: "excluded"; reason: string };

export interface LedgerFile {
  /** Repository-relative path, as `git ls-files` lists it at the base commit. */
  file: string;
  disposition: FileDisposition;
  /** Short note: what in this file defines, converts, sends or reads, and where. */
  note?: string;
}

/** The outside edges of the Router. */
export type OutsideEdge =
  | "cli-codex"
  | "managed-codex"
  | "acp-agent"
  | "acp-served-n8"
  | "acp-served-n9"
  | "mcp";

/** Where a send site's messages are recorded and checked, or that nothing does. */
export type SendSiteCoverage =
  | { kind: "tap"; testFile: string; cases: string[] }
  | { kind: "live"; recording: string }
  | { kind: "uncovered"; reason: string };

/**
 * Who a send belongs to: Router product code, or test apparatus (a fixture or verification driver
 * playing a counterparty). Apparatus sends are listed and counted apart from product coverage.
 */
export type SendOrigin = "product" | "apparatus";

export interface SendSite {
  file: string;
  line: number;
  edge: OutsideEdge;
  /** The method or message kind sent there. */
  sends: string;
  owner: Owner;
  origin: SendOrigin;
  coverage: SendSiteCoverage;
}

/** One crate's (or provider package's) share of the ledger. */
export interface LedgerSection {
  /** The crate or package directory, such as `crates/acp-client-runtime`. */
  root: string;
  files: LedgerFile[];
  sendSites: SendSite[];
}

export type FindingKind =
  | "violation"
  | "duplicate"
  | "hand-conversion"
  | "json-in-string"
  | "no-authoritative-definition"
  | "owned-disagreement"
  | "wrong-pin"
  | "unseen-kind"
  | "coverage-gap"
  | "not-exercised"
  | "part-defect"
  | "apparatus";

export interface Finding {
  kind: FindingKind;
  owner: ContractOwner;
  where: {
    edge?: string;
    part?: Part;
    crate?: string;
    file?: string;
    line?: number;
    /** For an unseen kind (R53): the pinned definition, the sending role and the method. */
    definition?: string;
    role?: string;
    method?: string;
  };
  detail: string;
  fixedBy: FixedBy;
}
