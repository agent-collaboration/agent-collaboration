import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/central-link-protocol/src/";
const centralLink =
  "Router-central link wire (module doc lib.rs:1 'the closed Router–central wire vocabulary'), not one of the six outside edges";

export const section: LedgerSection = {
  root: "crates/central-link-protocol",
  files: [
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations, re-exports and ProtocolError l9",
    },
    {
      file: `${R}link_codec.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralLink}: DataFrame l27 {welcome, request, reply, deliver, ack} and LinkFrame l14 with literal ping/pong text in decode_text l69 / encode_text l79`,
      },
    },
    {
      file: `${R}link_request.rs`,
      disposition: {
        kind: "excluded",
        reason: `${centralLink}: CorrelationId l9 (central transport correlation, validated as workspace_ids::RequestId l21, not a Router-vocabulary correlation id), RequestPayload l45, ReplyPayload l75, LinkRequest l93 via RequestWire l125`,
      },
    },
  ],
  sendSites: [],
};
