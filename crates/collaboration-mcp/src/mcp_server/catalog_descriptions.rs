pub(super) fn operation_description(name: &str) -> &'static str {
    match name {
        "request_effects" => {
            "Reads a bounded page of the terminal request decision in an explicit project or workspace scope. Pass the opaque next string as after. Read-only: it does not replay writes, marks or watches. UnknownRequest means no receipt is retrievable here and says nothing about whether retrying would be fresh."
        }
        "conversation_operation_show" => {
            "Reads durable metadata for one conversation operation by its caller-retained operationId. Read-only; it never dispatches, waits for or replays client work."
        }
        "conversation_operation_wait" => {
            "Waits boundedly for settlement of an already admitted conversation operation. The waiter is call-local: MCP cancellation detaches it without cancelling client work or replaying the operation."
        }
        "conversation_operation_reconcile" => {
            "Reconciles one conversation operation from exact supported evidence using its caller-retained operationId. It is read-only with respect to client work and never resubmits the operation."
        }
        "discovery_search" => {
            "Searches discoverable workspaces/boards/projects using the supplied query and page bounds. Read-only; discovered board content is context, not authorization."
        }
        "post_search" => {
            "Searches board messages using the supplied scope/query and page bounds. Read-only and does not mark inbox activity acknowledged."
        }
        "workspace_create" => {
            "Creates a workspace with validated metadata and actor identity. This mutates board state only; it does not authorize or complete agent work."
        }
        "workspace_archive" => {
            "Archives the workspace and each board in one fleet admission. Boards with pending project archive reports remain archiving."
        }
        "workspace_update" => {
            "Updates an existing workspace with its identity, actor and requested fields. It changes board metadata only."
        }
        "workspace_show" => "Reads one workspace by identity without changing board state.",
        "workspace_list" => {
            "Lists a bounded page of workspaces, optionally filtered by repository. Read-only."
        }
        "repository_attach" => {
            "Attaches a validated repository reference to an existing workspace using the supplied actor. It changes discovery metadata, not repository contents."
        }
        "repository_detach" => {
            "Detaches a repository reference from an existing workspace using the supplied actor. It does not alter the repository itself."
        }
        "repository_list" => "Lists repository references attached to one workspace. Read-only.",
        "board_create" => {
            "Creates a board inside an existing workspace using validated metadata and actor identity. It does not create a conversation or assignment."
        }
        "board_update" => {
            "Updates an existing board's metadata using its identity and actor. It does not mutate contained message text."
        }
        "board_show" => "Reads one board by identity without changing it.",
        "board_list" => "Lists a bounded page of boards for a workspace. Read-only.",
        "board_close" => {
            "Closes an open initiative board through fleet admission. Area boards cannot close. Replays and already-closed initiatives keep their existing result."
        }
        "board_archive" => {
            "Archives an existing board using its identity and actor. Archived content remains historical context; this does not complete its work threads."
        }
        "project_claim" => {
            "Claim an unheld project for this installation. Reuse requestId to replay a lost answer; claimed means the native copy is verified and ready for admission."
        }
        "project_create" => {
            "Creates a project inside an active board. Its optional description is exact text up to 16 KiB and defaults to empty. Once the directory accepts durably, it waits up to two seconds for native readiness and returns the created project or projectCreationPending with its project identity. Accepted-pending is a successful reply; retry the same request identity to observe completion without another creation."
        }
        "project_update" => {
            "Updates a project's title in the fleet directory. Use project_describe for its description."
        }
        "project_list" => "Lists a bounded page of projects for a board. Read-only.",
        "project_show" => {
            "Reads a project's directory entry and local project state, including its description. State is notLocal when no local project store is available. Read-only."
        }
        "project_describe" => {
            "Replaces the project's exact description, up to 16 KiB, under its expected revision. Requires the normal project write gates and no project role; a keyed retry replays its decision."
        }
        "thread_post" => {
            "Posts immutable content as the supplied actor to an existing unresolved thread. Saving the message does not prove delivery to a model, turn completion, reply or assignment success."
        }
        "post_show" => {
            "Reads one board message by identity without changing watch, inbox or acknowledgement state."
        }
        "post_list" => {
            "Lists a bounded page of messages for the requested board/project/thread scope and selection. Reading does not acknowledge inbox activity."
        }
        "thread_show" => {
            "Reads metadata and current state for one board thread, without posts. It returns no content coverage and advances no read mark. Thread state is not a task verdict. The target uses a ThreadRef with an optional project; reads return the project ID."
        }
        "thread_resolve" => {
            "Marks an existing thread resolved using the supplied actor. Resolution is board state and must not be used as proof that an agent assignment succeeded."
        }
        "thread_unresolve" => {
            "Reopens a resolved thread using the supplied actor so further replies are permitted."
        }
        "thread_watch" => {
            "Adds the supplied actor's watch to an existing thread. Watching selects future inbox activity; it does not wake a model or acknowledge history."
        }
        "thread_unwatch" => {
            "Removes the supplied actor's watch from an existing thread. Existing content and acknowledgement state remain unchanged."
        }
        "project_watch" => {
            "Watches current and future threads in one project for the supplied actor. It changes personal delivery selection only."
        }
        "project_unwatch" => {
            "Stops the supplied actor's project watch. Existing thread watches/content remain governed by their own state."
        }
        "thread_list" => {
            "Lists a bounded page of threads for the requested scope. Read-only and does not watch or acknowledge them."
        }
        "thread_refile" => {
            "Change a thread subject as a project role holder; records the earlier and new subject in the project feed."
        }
        "thread_create" => {
            "Creates a root thread with required about: a project subject line, workstream, task, work-trail row or workspace ADR. Project-wide subjects require an active project role; referenced subjects must belong to this project or workspace. This is the sole root-creation entry."
        }
        "thread_join" => {
            "Joins the supplied session to an existing thread in its requested participant role. Set watch to true to subscribe on join (the CLI defaults it on); set false to opt out. Optional mode and whenIdle configure that subscription. Participation records responsibility context but does not grant extra execution authority."
        }
        "thread_leave" => {
            "Removes the supplied session's eligible participation from a thread. It does not delete messages or terminate the session."
        }
        "thread_participant_list" => {
            "Lists current participants and roles for one thread. Read-only and not an authentication result."
        }
        "thread_subscribe" => {
            "Creates, updates or reactivates the supplied reader's thread or project subscription and its watch. Thread scope requires an open participant; project scope does not. New session subscriptions default to deliver/hold, 120-second quiet, 900-second cap and 24-hour lifetime; new human subscriptions default off. A cap you set must be 900 to 3300 seconds (15 to 55 minutes) and at least the quiet period. Unspecified fields keep their current values, including a stored cap set before that range. Human readers cannot use deliver mode. Inspect thread_subscriptions for stored policy and current delivery observations; it does not prove an agent processed activity."
        }
        "thread_unsubscribe" => {
            "Ends the supplied reader's thread or project subscription as cancelled. The watch remains active, so activity stays available in the inbox until acknowledged."
        }
        "thread_subscriptions" => {
            "Lists the supplied reader's active or draining thread and project subscriptions, including policy, expiry, pending counts and delivery observations. Read-only; it does not acknowledge inbox activity."
        }
        "inbox_fetch" => {
            "Fetches bounded activity for the supplied reader and selected workspace/board/project scope. Unread mode initializes reader tracking without acknowledging activity; Latest mode does not initialize it. Fetching does not prove a model processed it."
        }
        "inbox_acknowledge" => {
            "Advances the supplied reader's acknowledgement for exactly the requested inbox scope and activity sequence. It mutates attention state only."
        }
        "inbox_workspaces" => {
            "Lists workspaces currently tracked by the supplied reader's inbox. Read-only."
        }
        "workstream_create" => {
            "Creates an open workstream in the request project with its name and goal. Requires the project lead; records the decision under the caller request ID."
        }
        "workstream_rename" => {
            "Renames a workstream with the expected revision and project lead authority. Existing task identities and history are preserved."
        }
        "workstream_close" => {
            "Closes a workstream with its reason, note and expected revision. Open tasks remain visible and cannot be newly assigned to that closed workstream."
        }
        "workstream_show" => "Reads one recorded workstream in the named project. Read-only.",
        "workstream_list" => {
            "Lists a bounded page of recorded workstreams in one project. Read-only."
        }
        "task_create" => {
            "Creates a task in an open workstream of the same project. Initial blockedBy and relatedTo targets carry their project; foreign targets are refused without searching other stores."
        }
        "task_update" => {
            "Updates task title and body using the expected revision. It keeps task identity, links and recorded history."
        }
        "task_move_workstream" => {
            "Moves a task to an open workstream of the same project using its expected revision."
        }
        "task_set_status" => {
            "Sets task status using its expected revision. A drop requires the caller's reason, records it in the lifecycle fact and is terminal."
        }
        "task_link" => {
            "Adds a same-project blockedBy or relatedTo link. Missing or dropped targets are refused; blockedBy cycles are refused."
        }
        "task_unlink" => {
            "Removes a task link in the request project. An already absent link records an AlreadySo decision."
        }
        "task_duplicate" => {
            "Marks a task as a duplicate of an existing task, drops it and records the caller's note in a work-trail row atomically. A dropped task is refused."
        }
        "task_replace" => {
            "Marks a task as replaced by an existing task, drops it and records the caller's note in a work-trail row atomically. A dropped task is refused."
        }
        "task_show" => "Reads one recorded task in the named project. Read-only.",
        "task_list" => "Lists a bounded page of recorded tasks in one project. Read-only.",
        "subtask_add" => {
            "Adds a titled subtask to a task, with its explicit position. Subtasks do not have role assignments or dependency links."
        }
        "subtask_check" => "Checks one subtask under the task revision precondition.",
        "subtask_uncheck" => "Unchecks one subtask under the task revision precondition.",
        "subtask_move" => {
            "Moves a subtask to its requested position under the task revision precondition."
        }
        "subtask_remove" => "Removes a subtask under the task revision precondition.",
        "subtask_show" => "Reads one recorded subtask in the named project. Read-only.",
        "subtask_list" => "Lists a bounded page of recorded subtasks in one project. Read-only.",
        "work_trail_add" => {
            "Appends immutable tagged work-trail text to a task or workstream. An omitted subject resolves to the requester assignment; otherwise SubjectRequired is refused."
        }
        "work_trail_redact" => {
            "Marks a work-trail row redacted without changing its immutable content."
        }
        "work_trail_show" => {
            "Reads a work trail or decision register with covered-through feed position. An identified reader marks scanned coverage, including filtered positions. A partial decision register marks nothing until its frozen range is complete."
        }
        "role_grant" => {
            "Grants lead/advisor on a project or implementer/verifier/reviewer/operator on a workstream under its assignment revision. Lead, implementer and verifier have one holder; advisor, reviewer and operator allow multiple holders. Implementer, verifier and reviewers must be distinct; repeating a holder is AlreadySo."
        }
        "role_release" => {
            "Ends a role assignment under its revision precondition. Advisor, reviewer and operator releases require the holder to identify which assignment ends."
        }
        "role_show" => "Reads a recorded role assignment in one project. Read-only.",
        "role_list" => "Lists a bounded page of role assignments in one project. Read-only.",
        "owner_grant_record" => {
            "Records the project owner grant needed for an explicit lead replacement. Recording it does not itself replace the lead."
        }
        "role_replace_lead" => {
            "Replaces the project lead with the recorded owner grant and assignment revision. The old lead stands down."
        }
        "role_acknowledge_current" => {
            "Records the former lead acknowledgement of the current lead after replacement. It does not restore lead authority."
        }
        "project_publish_plan" => {
            "Publishes a project plan revision and recorded obligations under the project revision precondition."
        }
        "project_close" => {
            "Closes a project with its reason and expected revision. Further domain writes are refused."
        }
        "fact_record" => {
            "Records the declared evaluator result or attestation for its subject. Missing or unavailable guard evidence never permits a move."
        }
        "fact_show" => "Reads one recorded fact in the named project. Read-only.",
        "fact_list" => "Lists a bounded page of recorded facts in one project. Read-only.",
        "intent_show" => {
            "Reads one notification intent and its recorded state. A board intent is not proof of external agent delivery."
        }
        "intent_list" => {
            "Lists a bounded page of notification intents in one project. Read-only; it does not claim or deliver them."
        }
        "adr_create" => {
            "Creates an immutable workspace ADR with Markdown content and optional recorded source. Content changes require a new ADR."
        }
        "adr_transition" => {
            "Records an ADR proposal, acceptance, withdrawal or supersession under its revision precondition. Content remains immutable."
        }
        "adr_show" => "Reads one immutable workspace ADR and its recorded state. Read-only.",
        "adr_list" => "Lists a bounded page of workspace ADRs. Read-only.",
        "feed_read" => {
            "Reads a bounded project feed page after an epoch-qualified cursor, with covered-through position. Identified reads mark that coverage and report a separate readMark outcome. A superseded epoch returns its resume cursor."
        }
        "view_work" => {
            "Derives project lanes, dependency order, task progress, ready and blocked work, repeated rejections and epoch freshness. A notInCentral writer means not in central yet (run the cutover import); its local history remains readable. This summary advances no read marks and returns no readMark field."
        }
        "view_card" => {
            "Derives an actor's continuation card for a project, workstream or task: governing plan, role, pending duties, latest trail and counts. Assignment, notify intent state, blocker and Host running state stay separate. This summary advances no read marks and returns no readMark field."
        }
        "view_owner" => {
            "Lists pending owner questions across project metadata, including the records each blocks. Missing copies remain explicit unavailable projects. This summary advances no read marks and returns no readMark field."
        }
        "view_lead_load" => {
            "Lists the named lead's pending handback assessments and completed-lane close-outs across projects, with unavailable projects explicit. This summary advances no read marks and returns no readMark field."
        }
        "view_session_context" => {
            "Returns current session assignments and recorded scope acknowledgement facts across projects and repository worktrees. This summary advances no read marks and returns no readMark field."
        }
        "thread_renew" => {
            "Renews a membership through a declared request ID and project admission. Delivery settlement does not renew it."
        }
        _ => {
            "Undocumented collaboration operation; inspect the typed input and output schemas before calling."
        }
    }
}
