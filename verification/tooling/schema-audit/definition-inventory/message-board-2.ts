import type { InventoryRow } from "../schema-audit-types.ts";

/** message-board definitions the first file did not name: MCP results, record shapes and hand decoders. */
export const rows: InventoryRow[] = [
  {
    file: "crates/message-board/src/board_operations/message_operations.rs",
    line: 36,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results post_show PostShowResult L36 (serde transparent: the Post record itself) and post_list PostListResult L42 {page: PostPage}; catalog_tools.rs:261-262",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_operations/message_operations.rs",
    line: 54,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results thread_show ThreadShowResult L54 {projectId, readMark, flattened Thread fields, subject: ThreadSubjectView, watchStatus}, thread_list ThreadListResult L114 {page: Page<ThreadReadRecord>} and thread_participant_list ThreadParticipantListResult L166 {projectId, page: Page<Participant>, orchestrator, implementer}; catalog_tools.rs:263, 270",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_operations/setup_operations.rs",
    line: 45,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results workspace_show WorkspaceShowResult L45 {workspace}, workspace_list WorkspaceListResult L49 {page: Page<Workspace>} and repository_list RepositoryListResult L73 {page: Page<RepositoryRef>}; catalog_tools.rs:244-248",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_operations/setup_operations.rs",
    line: 98,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results board_show BoardShowResult L98 {board} and board_list BoardListResult L100 {page: Page<Board>}; catalog_tools.rs:251-252",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_operations/setup_operations.rs",
    line: 143,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results project_show ProjectShowResult L143 {project: ProjectDirectoryEntry, state: ProjectStateView} with ProjectStateView L155 {kind local, value ProjectState | notLocal}, and project_list ProjectListResult L139 {page: Page<ProjectDirectoryEntry>}; catalog_tools.rs:258-259",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_operations/inbox_operations.rs",
    line: 107,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results inbox_fetch InboxFetchResult L107 {page: InboxPage}, inbox_acknowledge InboxAcknowledgeReply L204 {outcomes} with AcknowledgementEntry L108, and inbox_workspaces InboxWorkspacesResult L212 {page: Page<WorkspaceUnreadSummary>}; catalog_tools.rs:279-281",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_search.rs",
    line: 56,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results discovery_search DiscoverySearchResult L73 of DiscoveryHit L56 (tag kind) and post_search PostSearchResult L98 of LocatedPost L91; catalog_tools.rs:239-240",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/entity_operations.rs",
    line: 80,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP results WorkTrailReadResult L80 (work_trail_show, inside BoardReadReply) and RecordListResult L87 (every *_list record read); catalog_tools.rs:217-238",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/read_mark_outcomes.rs",
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ReadActor L14 (anonymous | identified{reader}, carried by ThreadShowRequest), ReadMarkNotAdvanced L35, ReadMarkOutcome L50 and BoardReadReply<T> L57 {value, readMark}, the read reply wrapper of work_trail_show and the read mark in ThreadShowResult",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_views.rs",
    line: 56,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP view results ContinuationCard L56 (view_card), WorkView L81 with WorkstreamLane, WorkTask, BlockedTask and the writer observation (view_work), OwnerView L99 with OwnerWait (view_owner), LeadLoad L102 with LeadOwed (view_lead_load) and SessionContext L141 (view_session_context); defined by the view! macro (L15)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_messages.rs",
    line: 249,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "board message shapes returned by MCP reads: Post L249, Thread L266, HistoryRange L278, WatchStatus L285, InboxActivity L294, WorkspaceUnreadSummary L317, Bookmark L325, InboxInitializationStatus L333, PostPage L346 and InboxPage L359 (both with an opaque nextCursor and the structured AggregateCursor next); request shapes ReadScope L145, InboxScope L154, PostListScope L165, ReferenceTarget L136 and Placement L125",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/board_messages.rs",
    line: 202,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PostSelectionWire L202, a hand-written second definition of PostSelection L187 (latest | afterPosition{after} | range{from, to}) used only to decode it (L215-226) before validation; the advertised schema is derived from PostSelection, so a variant added there and not here is advertised but refused",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "post-selection-encoding",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/message-board/src/board_metadata.rs",
    line: 134,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "board directory records returned by MCP reads: Workspace L134, Board L145 and ProjectDirectoryEntry L157, with BoardState L98, WorkspaceState L107, BoardKind L120 and ThreadState L127",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/project_feed.rs",
    line: 39,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "Record L39 (tag kind, 23 record variants), RecordScope L15, RecordEnvelope L23, FeedContent L176, FeedEntry L183, FeedRead L201 (feed_read), TrailPage L214, DecisionPage L222, ReadMark L231, RequestLedgerEntry L240, EpochLine L250, PlanRevision L258, IntentState L273, IntentRefusal L287 and NotifyIntent L327 with its recipients (L295-316): the record payloads of RecordListResult and FeedRead",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/work_records.rs",
    line: 32,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "work records carried in Record and BoardOperation: Workstream L32, TaskStatus L44, TaskStatusChange L58 (target_status maps drop to dropped, L65-73), Task L78, TaskLink L100, TaskLinkEvent L120, Subtask L128, WorkTrailRow L153, TrailRedaction L173 and TaskAction L186",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/fact_records.rs",
    line: 20,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "fact records: EvidenceRef L20, ReceiptFact L56, LifecycleFact L109, FactKind L147, Fact L181 (a Record variant) and FactInput L194 (the fact_record BoardOperation input)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/owner_grants.rs",
    line: 13,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "owner records carried in Record: GrantKind L13, OwnerWordsProvenance L28, OwnerGrant L34 and StandDownAcknowledgement L43",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/project_state.rs",
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ProjectState L8, the project Record variant and the local value of ProjectStateView (setup_operations.rs:155)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/subscription_membership.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "SubscriptionMembership L37 (validated from SubscriptionMembershipProps L20, with MembershipEnd L12), a Record variant",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/workspace_adrs.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ADR records: AdrStatus L12, Adr L21 (a Record variant returned by adr_show/adr_list) and AdrTransition L41 (adr_transition input)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/writer_observation.rs",
    line: 28,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "WriterObservation L64 {lease version L28, registry incarnation L12}, carried in WorkView (board_views.rs:86) and filled from central reads (collaboration-runtime central_project_reads.rs:87)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_subscriptions.rs",
    line: 131,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "subscription shapes used by subscription requests and replies: BatchTiming L131, SubscriptionLifetime L211, SubscriptionPolicy L257, SubscriptionTimingPatch L392, SubscriptionPolicyPatch L399, SubscriptionState L542, SubscriptionRootRecord L583 and ThreadSubscriptionRecord L694",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_subscriptions/root_notice.rs",
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PendingRootNotice L14 (returned in the thread wait roots, collaboration-protocol thread_subscription_wait.rs:114, 121), SubscriptionBatch L60 and its settlements L75, L85",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_references.rs",
    line: 57,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "reference shapes in requests and results: PostTarget L57, SubscriptionTarget L70, ReadScopeRef L105 and ThreadReadRecord L126 (thread_list entries)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_refusals.rs",
    line: 10,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ThreadRefusal L10 (tag kind, ten seat refusals); its decoder ThreadRefusalWire L14 is generated by the same thread_refusal_shapes! field list (L6-24) and then validated, so it is one definition, not a copy",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_participants.rs",
    line: 93,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "Participant L93 (thread participant output) decoded through ParticipantWire L118, a hand-written second list of the same eight fields, converted to ParticipantProps and validated (L130-144)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "participant-encoding",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/message-board/src/thread_lifecycle.rs",
    line: 45,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines: "LeaveFollowUp L45, carried by the thread_leave request (ThreadLeaveRequest)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/thread_subjects.rs",
    line: 52,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ThreadSubject L52, SubjectTitle L66, ThreadSubjectView L73 (ThreadShowResult.subject) and ThreadRefiled L79 (a Record variant)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/record_references.rs",
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RecordKind L14 and RecordRef L41 {kind, id}, and EvidencePointer L146: the record reference shape in results and refusals",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/repository_identity.rs",
    line: 107,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RepositoryRef L107 (tagged repository identity in workspace and repository requests and results)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/project_requests.rs",
    line: 250,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "TaskLinkInput::link_in_project: a qualified TaskLinkTarget request input converted to the stored TaskLink {from, kind, to: task in this project} (L250-257)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/aggregate_reads.rs",
    line: 10,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "AggregateCursor L10 {perProject, sweep: MergeKey}, MergeKey L17 and MergeTime L32 (tag kind, unknown | known): the structured cross-project page position in PostPage and InboxPage next; From<RecordTime> maps LegacyUnknown to Unknown and Router(time) to Known (L37-43)",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/record_positions.rs",
    line: 101,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FeedCursor L101 {epoch, position} and RecordPlacement L113 (project{cursor, origin} | fleet{position, origin} | unsequenced): feed positions carried in records, read marks and acknowledgements",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/message-board/src/record_time.rs",
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RecordTime L14 (legacyUnknown | router(time)), the record time carried in record envelopes",
    usedBy: [
      { part: "N11" },
      { crate: "collaboration-client" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
