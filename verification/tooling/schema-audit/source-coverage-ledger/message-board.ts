import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 8 colocated cfg(test) unit-test files, each declared under #[cfg(test)]:
// abridged_evidence_test.rs (abridged_evidence.rs:189), archive_actor_tests.rs (lib.rs:96),
// board_views_test.rs (board_views.rs:52), mail_item_reference_tests.rs (mail_item_reference.rs:46),
// repository_identity_test.rs (repository_identity.rs:153), subscription_membership_tests.rs (lib.rs:38),
// thread_refusals_test.rs (thread_refusals.rs:62), thread_subscriptions_tests.rs (lib.rs:51).
// The crate has no IO dependency (Cargo.toml: schemars, serde, serde_json, thiserror, uuid, chrono,
// sha2, petgraph, url) and writes nothing on any outside edge, so it has no send sites.

export const section: LedgerSection = {
  root: "crates/message-board",
  files: [
    {
      file: "crates/message-board/src/abridged_evidence.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "AbridgedEvidence L95 with validating EvidenceWire L105; hand-built Assignment evidence {kind, subject} L31; canonical_object_order L61 re-serializes (RecordRef, RecordPlacement) through json! L57 for the evidence digest L80.",
    },
    {
      file: "crates/message-board/src/aggregate_reads.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "AggregateCursor L10, MergeKey L17, MergeTime L32 (From<RecordTime> L37, board-internal), ReadPosition L48, AggregatePage L75: cursor and page shapes inside board read replies.",
    },
    {
      file: "crates/message-board/src/authority_roles.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RecordedActor L25 (carries board SessionRef) with Identity copies L80, L88, L101; ActorKind L40 through ActorKindWire L47; AuthorityRole L127 with as_str table L152; Assignment L196; RoleAssignment L319 via RoleAssignmentProps L304.",
    },
    {
      file: "crates/message-board/src/board_failures.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "BoardError L160 {kind BoardFailureKind L10, stage L47, message, nextAction L56, details BoardErrorDetails L90}: the board failure body; constructors build fixed messages (participants_required L193 embeds a CLI command line).",
    },
    {
      file: "crates/message-board/src/board_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "id patterns L7-L12; uuid_v7_identity! L21 board ids; PostId/ThreadId From L87; record_identity! L98; validated_identity_string! L117 ServiceId/EndpointId/SessionId/HumanId; SessionEndpointRef L202 and SessionRef L211 (copies of collaboration-protocol endpoint_identity.rs:94, 107); Identity L220 with short_form L237.",
    },
    {
      file: "crates/message-board/src/board_messages.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "PostReferences L17 with manual JsonSchema L27 via PostReferencesSchema L25; ActivitySequence L80 manual schema L99; Post L249, Thread L266, InboxActivity L294, PostPage L346, InboxPage L359 (nextCursor opaque strings); PostSelection L187 decoded through PostSelectionWire L202.",
    },
    {
      file: "crates/message-board/src/board_metadata.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "bounded_text! L23 (ResourceName, Description, PostText, ... MachineLabel L64-L80) with hand-written JSON Schema fragments (pattern, allOf) via json! L33, L38; Workspace L134, Board L145, ProjectDirectoryEntry L157. Inline cfg(test) mod at L82.",
    },
    {
      file: "crates/message-board/src/board_operations/inbox_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs InboxFetchRequestProps L27 / InboxFetchRequest L39, InboxAcknowledgeRequestProps L116 / L129, InboxWorkspacesRequest L206 (collaboration-mcp catalog_tools.rs:279-281); ProjectWriteCallResult L196 wraps the public WriteReply.",
    },
    {
      file: "crates/message-board/src/board_operations/message_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs and results for thread_* and post_* tools: ThreadPostRequest L20, PostShowResult L36, ThreadShowResult L54, ThreadCreateRequest L116, ThreadCreatorParticipation L133, ThreadJoinRequest L141, ThreadRenewRequest L173.",
    },
    {
      file: "crates/message-board/src/board_operations/mod.rs",
      disposition: { kind: "none" },
      note: "Module list and glob re-exports of the three operation modules; no shapes.",
    },
    {
      file: "crates/message-board/src/board_operations/setup_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs and results for workspace_*, repository_*, board_* and project_* setup tools (WorkspaceCreateRequest L16 through ProjectShowResult L143); ProjectStateView L155.",
    },
    {
      file: "crates/message-board/src/board_pagination.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "PageLimit L13 with manual JsonSchema L24; PageRequest L51 and Page L60 (cursor and nextCursor are opaque strings).",
    },
    {
      file: "crates/message-board/src/board_search.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs DiscoverySearchRequest L44 and PostSearchRequest L79; DiscoveryScope L11, DiscoveryHit L56, results L73, L98.",
    },
    {
      file: "crates/message-board/src/board_views.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "view! results ContinuationCard L56, WorkView L81, OwnerView L99, LeadLoad L102; CopySource L30, CurrentWriter L42, SessionRunningState L75, UnavailableReason L126 and SessionContext L141 (carry board SessionRef), IntentClaim L156.",
    },
    {
      file: "crates/message-board/src/decision_effects.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Native (raw keyset) receipt shapes: DecisionEffectsHeader L33, RequestEffectsScope L63, RequestEffectsCursor L70, RequestEffectsRequest L77, RequestEffectsRead L93, DecisionEffectsPage L100 via EffectsPageWire L110; public copies in public_replies.rs.",
    },
    {
      file: "crates/message-board/src/entity_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs ProjectRecordShowRequest L7, ProjectRecordListRequest L13, SubtaskListRequest L19, FleetRecordShowRequest L27, FleetRecordListRequest L32, FeedReadRequest L38, WorkTrailShowRequest L47; WorkTrailReadResult L80, RecordListResult L87; ProjectReadKind to RecordKind mapping L102.",
    },
    {
      file: "crates/message-board/src/fact_bounds.rs",
      disposition: { kind: "none" },
      note: "validate_bounds byte and count limits on ArtifactRef, EvidenceRef and FactKind (L8-L77); defines and converts no shape.",
    },
    {
      file: "crates/message-board/src/fact_records.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "EvidenceRef L20, ArtifactRef L33, ReceiptFact L56 (executor board SessionRef), PrFactKind L87, HookKind L95, LifecycleFact L109, FactKind L147 (hookFact and scopeAcknowledgement carry board SessionRef), Fact L181, FactInput L194.",
    },
    {
      file: "crates/message-board/src/fleet_operations.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs AdrCreateRequest L10 and AdrTransitionRequest L25.",
    },
    {
      file: "crates/message-board/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and re-exports; declares the cfg(test) modules at L38, L51, L96.",
    },
    {
      file: "crates/message-board/src/mail_item_reference.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MailItemRef L9: a mail-owned item id held as a board RecordId (TryFrom L11), manual JsonSchema L37 renaming the RecordId schema to ProjectMailItemRef.",
    },
    {
      file: "crates/message-board/src/owner_grants.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "GrantKind L13, OwnerWordsProvenance L28, OwnerGrant L34, StandDownAcknowledgement L43.",
    },
    {
      file: "crates/message-board/src/project_feed.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RecordEnvelope L23, Record L39 (tag kind, content record) with Record::reference hand mapping to RecordRef L122, FeedContent L176, FeedEntry L183, FeedRead L201, NotifyIntent L327 via NotifyIntentProps L337.",
    },
    {
      file: "crates/message-board/src/project_requests.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ProjectRequest L15 and BoardOperation L31 (41 variants): the MCP input of every project write tool (catalog_tools.rs:188-214); TaskLinkTarget L227, TaskLinkInput L244.",
    },
    {
      file: "crates/message-board/src/project_state.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ProjectState L8 record.",
    },
    {
      file: "crates/message-board/src/project_tool_selectors.rs",
      disposition: { kind: "defines", owners: ["mcp", "router-vocabulary"] },
      note: "project_tools! L39-L98: 27 MCP tool names with the BoardOperation kind literal each accepts; TaskActionSelection::kind L13 and operation_kind L30 literals copy the serde tags.",
    },
    {
      file: "crates/message-board/src/public_replies.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "Public MCP receipt shapes (catalog_tools.rs:164): RequestEffectsRequest L14, DecisionEffectsPage L22 via EffectsPageWire L33, from_native L79, RequestDecision L116, RequestResult L134, WriteReply L165, RequestEffectsRead L172; copies of decision_effects.rs and request_decisions.rs natives. Inline cfg(test) mod at L177.",
    },
    {
      file: "crates/message-board/src/read_mark_outcomes.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ReadActor L14, ReadMarkNotAdvanced L35, ReadMarkOutcome L50, BoardReadReply L57.",
    },
    {
      file: "crates/message-board/src/record_bounds.rs",
      disposition: { kind: "none" },
      note: "Byte and count limits (text_bound L13, count_bound L26) applied by validate_bounds on EvidencePointer, WorkTrailRow, Record and Refusal; defines and converts no shape.",
    },
    {
      file: "crates/message-board/src/record_positions.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "numeric_value! L14 with manual JsonSchema L36 for FeedPosition, FleetPosition, HandoffSeq, CancelExpansionSeq, EpochNumber, SubjectRevision, AssignmentRevision, ProtocolVersion, PostCooldownSeconds (L45-L53); FeedCursor L101, RecordPlacement L113.",
    },
    {
      file: "crates/message-board/src/record_references.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RecordKind L14, RecordRef L41, digest_value! L46 (CanonicalHash, SchemaFingerprint, PlanRevisionHash, CommitSha) with manual JsonSchema L62, EvidenceUrl L107 manual schema L130, EvidencePointer L146.",
    },
    {
      file: "crates/message-board/src/record_time.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RecordTime L14 {kind router|legacyUnknown, value}.",
    },
    {
      file: "crates/message-board/src/repository_identity.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "NormalizedOrigin L18 (manual JsonSchema L41), CommonDirectory L63 (manual JsonSchema L83), RepositoryRef L107; normalize_git_origin_url L118 is a URL normalizer, not a message parser.",
    },
    {
      file: "crates/message-board/src/request_canonical.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "canonical_hash L30 hashes the serde_json bytes of private CanonicalProjectRequest L22; creation_hash L45 hashes a positional JSON tuple; canonical_operation L66 sorts taskCreate links.",
    },
    {
      file: "crates/message-board/src/request_decisions.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Refusal vocabulary (WorkstreamRefusal L27, TaskRefusal L42, TrailRefusal L68, RoleRefusal L88, Refusal L110, SchemaRefusal L189, ScopeRefusal L230, ProjectRejection L255) and native RequestDecision L155, StoredDecision L172, RequestResult L209, WriteReply L276.",
    },
    {
      file: "crates/message-board/src/subscription_membership.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MembershipEnd L12, SubscriptionMembershipProps L20, SubscriptionMembership L37 record (validated through props).",
    },
    {
      file: "crates/message-board/src/subtask_order.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "SubtaskPlaceError L12, SubtaskPlace L33 hex key with hand hex codec L35, L64 and manual JsonSchema L70.",
    },
    {
      file: "crates/message-board/src/thread_lifecycle.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ThreadResolutionReason L9 (manual JsonSchema L34), LeaveFollowUp L45.",
    },
    {
      file: "crates/message-board/src/thread_participants.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ParticipantRole L10, RecordedParticipantRole L26, ParticipantClosedReason L33, ParticipantNote L47 (manual JsonSchema L77 counts characters, decoder counts bytes), Participant L93 via ParticipantWire L118, OrchestratorHolder L194, ImplementerHolder L201.",
    },
    {
      file: "crates/message-board/src/thread_references.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ThreadRef L10 with text form FromStr L23 / Display L41, PostTarget L57, SubscriptionTarget L70 and ReadScopeRef L105 with stored-scope conversions L76, L111, L133, L143, ThreadReadRecord L126.",
    },
    {
      file: "crates/message-board/src/thread_refusals.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ThreadRefusal L10 decoded through ThreadRefusalWire L14 (same variants, validated), ThreadRefusalValidationError L43.",
    },
    {
      file: "crates/message-board/src/thread_subjects.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "SubjectLine L11 (manual JsonSchema L36), ThreadSubject L52, SubjectTitle L66, ThreadSubjectView L73, ThreadRefiled L79.",
    },
    {
      file: "crates/message-board/src/thread_subscriptions.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "SubscriptionScope L52, SubscriptionMode L89, WhenIdle L108, BatchTiming L131, SubscriptionPolicy L257, SubscriptionPolicyPatch L399, EndReason L409 (as_str tables L71, L97, L116, L419), uuid_v7_id! L472, SubscriptionDeliveryOutcome L569 (untyped evidence), ThreadSubscriptionRecord L694, ThreadSubscriptionSubscribeRequest L843 / UnsubscribeRequest L851.",
    },
    {
      file: "crates/message-board/src/thread_subscriptions/root_notice.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "PendingRootNotice L14, SubscriptionBatch L60, SubscriptionBatchRootSettlement L75, SubscriptionBatchSettlement L85.",
    },
    {
      file: "crates/message-board/src/view_requests.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "MCP tool inputs ProjectViewRequest L8, ContinuationCardRequest L16, SessionContextRequest L24 (board SessionRef), OwnerViewRequest L30, LeadLoadRequest L34.",
    },
    {
      file: "crates/message-board/src/work_records.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "CloseReason L13, WorkstreamState L25, Workstream L32, TaskStatus L44, TaskStatusChange L58, Task L78, TaskLink L100, TaskLinkEvent L120, Subtask L128, TrailSubject L145, WorkTrailRow L153, TaskAction L186.",
    },
    {
      file: "crates/message-board/src/work_rules.rs",
      disposition: { kind: "none" },
      note: "Pure decisions over a project snapshot (role, link, trail subject rules); returns existing refusal values; defines and converts no shape.",
    },
    {
      file: "crates/message-board/src/workspace_adrs.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "AdrStatus L12, Adr L21, AdrTransition L41.",
    },
    {
      file: "crates/message-board/src/writer_observation.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "RegistryIncarnation L12, LeaseVersion L28, WriterObservation L64. Inline cfg(test) mod at L69.",
    },
  ],
  sendSites: [],
};
