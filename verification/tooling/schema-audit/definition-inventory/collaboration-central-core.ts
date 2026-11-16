import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-central-core/src/fleet_column_codec.rs",
    line: 107,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "ActorColumns::encode flattens RecordedActor::Session's board SessionRef to (service, endpoint, session) strings for every Fleet actor column set (adr, board, directory, workspace, feed, ledger stores)",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/fleet_column_codec.rs",
    line: 148,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "ActorColumns::decode (l134) rebuilds RecordedActor::Session's board SessionRef {endpoint {serviceId, endpointId}, sessionId} from (service, endpoint, session) columns; shared by the adr, board, directory, workspace, feed-header and ledger codecs",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/fleet_identity_columns.rs",
    line: 23,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "IdentityColumns::encode flattens message_board Identity::Session's SessionRef to (service, endpoint, session) strings for refusal actor/holder/target/named_holder columns (fleet_reason_write.rs:60-63)",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/fleet_identity_columns.rs",
    line: 46,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "IdentityColumns::decode rebuilds a message_board SessionRef {endpoint {serviceId, endpointId}, sessionId} from (service, endpoint, session) columns for refusal identities (fleet_reason_fields.rs:202-301)",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/lease_projection.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "LeaseProjectionVersion {epoch: fleet Epoch, incarnation, change: NonZeroU64}, camelCase deny_unknown_fields: second copy of message_board::LeaseVersion {epoch, incarnation, change} (writer_observation.rs:28); also serialized in LeaseProjectionResult Older/Conflict (:208-209)",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
      {
        part: "N11",
      },
    ],
    duplicateGroup: "lease-version-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/lease_projection.rs",
    line: 161,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "directory_metadata: hand conversion LeaseProjectionVersion -> message_board::LeaseVersion (epoch.value() try_into EpochNumber, RegistryIncarnation::new(incarnation.value()), change.get()), every failure collapsed to one corrupt directory.lease_version; result is LeaseMetadata.lease_version that the Router puts in MCP view_work writerObservation",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
      {
        part: "N11",
      },
    ],
    duplicateGroup: "lease-version-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/lease_version.rs",
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RegistryIncarnation(NonZeroU64), transparent: second copy of message_board::RegistryIncarnation (writer_observation.rs:12), same shape and error text; mapped by hand at lease_projection.rs:167 into the MCP view_work leaseVersion",
    usedBy: [
      {
        crate: "collaboration-central-core",
      },
      {
        part: "N11",
      },
    ],
    duplicateGroup: "lease-version-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-central-core/src/registration_refusal.rs",
    line: 7,
    owner: "mcp",
    kind: "hand-json",
    defines:
      'render derives fleet GateReason.code from message_board::Refusal\'s serde tag via serde_json::to_value(reason).get("kind"); the Router copies it verbatim into ProjectRegistrationRejection.code (collaboration-service project_rejection.rs:13), an MCP WriteReply output field',
    usedBy: [
      {
        part: "N11",
      },
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "mcp-tool-error-bodies",
    disposition: "malformed",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-central-core/src/registration_refusal.rs",
    line: 31,
    owner: "mcp",
    kind: "json-in-string",
    defines:
      "render puts serde_json::to_string(Refusal) into GateReason.detail for Workstream/Task/Trail/Role/Thread/StaleRevision/TopLevelPostCooldown/Closed/ProjectRejected; the Router copies detail into ProjectRegistrationRejection.detail (string) in MCP WriteReply RequestDecision::ProjectRejected",
    usedBy: [
      {
        part: "N11",
      },
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "json-in-string",
    disposition: "malformed",
    fixedBy: "D1",
  },
];
