//! Success counts and scope limits for each completed Worker scenario.

use super::{Scenario, forwarded_post_scenarios::ForwardCase};

impl Scenario {
    pub(super) fn print_success(self) {
        match self {
            Scenario::MailboxReportTerminal => println!(
                "MAILBOX_REPORT_TERMINAL_RESULT 1 passed; 0 failed; 0 ignored (real SDK report task and central socket; terminal refusal stops retries)"
            ),
            Scenario::MailboxHeldRetention => println!(
                "MAILBOX_HELD_RETENTION_RESULT 1 passed; 0 failed; 0 ignored (same Native/Worker oracle, actual held payload across31controlled days and exact receipt release; no physical restart claim)"
            ),
            Scenario::MailboxCapacity => println!(
                "MAILBOX_CAPACITY_RESULT 1 passed; 0 failed; 0 ignored (shared one-item quota, unknown rejected item, exact original and reserved cancel; no disk-full claim)"
            ),
            Scenario::MailboxCancel => println!(
                "MAILBOX_CANCEL_RESULT 1 passed; 0 failed; 0 ignored (real dropped custody answer, pending cancel readback, recipient decision and final original status; no Router end-to-end claim)"
            ),
            Scenario::MailboxIdentity => println!(
                "MAILBOX_IDENTITY_RESULT 1 passed; 0 failed; 0 ignored (shared native/workerd oracle, real mailbox replay/conflict and exact single original payload; no Router end-to-end claim)"
            ),
            Scenario::MailboxDeadline => println!(
                "MAILBOX_DEADLINE_RESULT 1 passed; 0 failed; 0 ignored (shared normal session, offline recipient, real alarm, newer status and released payload; no physical restart claim)"
            ),
            Scenario::DeliveryConfigRefusals => println!(
                "DELIVERY_TEMPLATE_REFUSALS_RESULT 2 passed; 0 failed; 0 ignored (real Worker missing relay field and malformed peer refusal)"
            ),
            Scenario::ClaimArchived => println!(
                "CLAIM_ARCHIVED_RESULT 1 passed; 0 failed; 0 ignored (shared Native/Worker scenario; actual Archived unretired imported seed, normal Claim exact board_archived refusal and owning Fleet reservation absent before/after; no D1b HTTP import or physical restart claim)"
            ),
            Scenario::ForwardReopen => println!(
                "FORWARD_REOPEN_RESULT 1 passed; 0 failed; 0 ignored (two actual owned Worker process lifetimes; stopped persistence known-correlation presence/opaque absence, sixteen retained slots before original deadline and sixteen fresh deliveries after expiry; public capacity observation, no private SQL read, DO eviction/hibernation or real Router writer claim)"
            ),
            Scenario::ForwardPost(ForwardCase::Capacity) => println!(
                "FORWARD_CAPACITY_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual sender Registry-dispatch barrier, reserved sixteenth slot, exact seventeenth Overloaded and terminal capacity reuse; no process restart or full row19 claim)"
            ),
            Scenario::ForwardPost(ForwardCase::Deadline) => println!(
                "FORWARD_DEADLINE_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual owning sender clock/alarm, original Unanswered and late-reply isolation from fresh identity; no process restart or full row19 claim)"
            ),
            Scenario::ForwardPost(ForwardCase::ExactBoundary) => println!(
                "FORWARD_BOUNDARY_RESULT 1 passed; 0 failed; 0 ignored (shared Native/Worker literal29.999s answer and30s exact original expiry, reserved capacity and independent fresh correlation; no physical restart or full row19 claim)"
            ),
            Scenario::ForwardPost(ForwardCase::TerminalRelease) => println!(
                "FORWARD_TERMINAL_RESULT 1 passed; 0 failed; 0 ignored (same Native/Worker oracle; normal own-holder and foreign-holder registration, fifteen retained requests across SenderHolds/NotHeld/answer/deadline slot release; no route-overlap or full row19 claim)"
            ),
            Scenario::ForwardPost(ForwardCase::RouteOverlap) => println!(
                "FORWARD_OVERLAP_RESULT 1 passed; 0 failed; 0 ignored (real held route expires, fresh correlation admitted, completion witness then raw owning read, no resurrection or duplicate terminal; no restart claim)"
            ),
            Scenario::ForwardPost(ForwardCase::OwnState) => println!(
                "FORWARD_STATE_RESULT 1 passed; 0 failed; 0 ignored (normal sockets and terminal reply, owner-only raw own forwarding tables contain no content/correlation; no global database scan or restart claim)"
            ),
            Scenario::ForwardPost(ForwardCase::AlarmEvent) => println!(
                "FORWARD_REAL_TIMER_RESULT 1 passed; 0 failed; 0 ignored (literal production30s, controlled clock and actual alarm, exact Unanswered, sixteen reusable fresh slots and late-reply isolation; no physical restart or full row19 claim)"
            ),
            Scenario::ForwardPost(_) => println!(
                "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)"
            ),
            Scenario::ClaimRetiring => println!(
                "CLAIM_RETIRING_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal owner CopyMismatch retirement; cached Register overridden, old Claim NotFound replay retained, fresh Claim exact transfer refusal, normal Directory Unheld; no private publication-ack or physical restart claim)"
            ),
            Scenario::ClaimRetired => println!(
                "CLAIM_RETIRED_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal completed owner retirement; Retired outranks cached Register, cached Claim and fresh Claim; no D1b admin import or physical restart claim)"
            ),
            Scenario::ImportedClaim => println!(
                "IMPORTED_CLAIM_RESULT 2 passed; 0 failed; 0 ignored (conformance Open import/unheld/seed on actual composed stores; normal owner retirement refusal with unchanged seed, normal exact Claim/replay/fresh AlreadyHeld, Fleet Directory and Copy fence; no D1b HTTP import, private publication-ack read, Retiring/Retired precedence or physical restart claim)"
            ),
            Scenario::ArchiveReports => println!(
                "ARCHIVE_REPORT_RESULT 1 passed; 0 failed; 0 ignored (same retained archive-report oracle as Native; actual normal Worker sessions, owning Fleet clock, exact first-report/replay/board/workspace/feed/ledger observations; no reservation-race or hosted claim)"
            ),
            Scenario::HolderSchemaOverlap => println!(
                "HOLDER_SCHEMA_OVERLAP_RESULT 1 passed; 0 failed; 0 ignored (same retained normal-schema/holder overlap case selected alone for a bounded correctness check)"
            ),
            Scenario::HolderLifecycle => println!(
                "HOLDER_LIFECYCLE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions and real Registry holder journal; readiness/expiry/replacement, exact frame/connection, close/alarm/late answer, held schema publication; fifth group combines existing hub primitive after normal admission with changed-SessionId wire close)"
            ),
            Scenario::SessionFailures => println!(
                "SESSION_FAILURE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions, targeted real postcommit publication failure, same-socket Register refusal, callback/SQL rollback and real retry, exact public journal/ordered socket barrier)"
            ),
            Scenario::SessionCapabilities => println!(
                "SESSION_CAPABILITY_RESULT 2 passed; 0 failed; 0 ignored (normal enrollment and session, real workers-rs attachment roundtrip and original origin, actual same-Connection-object alarm; old SqlTx pointer identity retired)"
            ),
            Scenario::OwnerRetirement => println!(
                "OWNER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (conformance import/unheld/seed setup on actual composed stores, normal HTTP retirement, before-row/read-after-retired, exact Fleet acknowledgement and stable receipt/replay/Migrate; D1b HTTP import, physical file row count and process restart unobserved)"
            ),
            Scenario::OwnerRegistry => println!(
                "OWNER_REGISTRY_RESULT 3 passed; 0 failed; 0 ignored (normal Fleet creation, Page/Migrate, holder Release/Copy fence/target renewal, unchanged preview/tamper/takeover/replay and held-lease refusal; no successful retirement or physical restart claim)"
            ),
            Scenario::CopyOwner => println!(
                "COPY_OWNER_RESULT 1 passed; 0 failed; 0 ignored (owner HTTP Health before initialization, target bytes, empty Page, real Repair, machine refusal and minimal retired receipt; fixture Copy creation/retirement, no migration-fault, populated-page or full Registry admin claim)"
            ),
            Scenario::FleetNotices => println!(
                "FLEET_NOTICES_RESULT 1 passed; 0 failed; 0 ignored (three normal admitted sessions; two reader installations with two exact numbered hints each, Fleet mutation and replay, own alarm, real Delivery and positioned notice read; no physical restart claim)"
            ),
            Scenario::DeliveryConfig { .. } => println!(
                "DELIVERY_CONFIG_RESULT 1 passed; 0 failed; 0 ignored (explicit relay distinct from receiver, exact first/replay/update/readmission answers through real Registry and Mailbox; no Router delivery-call closure claim)"
            ),
            Scenario::CreationGates => println!(
                "CREATION_GATES_RESULT 1 passed; 0 failed; 0 ignored (normal enrolled sessions and real Fleet/Registry/Copy; four creation gates, same-attempt retry/replay, first piece and held Claim refusal; no imported positive Claim or physical restart claim)"
            ),
            Scenario::HttpBodyKeepalive => println!(
                "HTTP_BODY_KEEPALIVE_RESULT 1 passed; 0 failed; 0 ignored (actual production enroll route, exactly2MiB unauthorized body with withheld suffix, two sends on one physical TCP connection, no unauthorized peer reservation)"
            ),
            Scenario::HttpBodyOverLimit => println!(
                "HTTP_BODY_OVER_LIMIT_RESULT 1 passed; 0 failed; 0 ignored (eight actual HTTP route/auth/method variants,2MiB+1 body,413 and physical close for each; one grouped scenario)"
            ),
            Scenario::RawSubmitRefusal => println!(
                "RAW_SUBMIT_RESULT 1 passed; 0 failed; 0 ignored (normal admitted sockets, permanent raw Submit refusal, same-socket survival and empty recipient custody)"
            ),
            Scenario::SenderSettlement => println!(
                "SENDER_SETTLEMENT_RESULT 1 passed; 0 failed; 0 ignored (normal Query and Settle through Registry to recipient Mailbox, known custody replay, unknown tombstone, late Submit refusal and custody readback)"
            ),
            Scenario::SenderRetirement => println!(
                "SENDER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (verified sender fencing, actual recipient Mailbox, same-CallId receipt replay and one control effect)"
            ),
            Scenario::CopyReads => println!(
                "COPY_READ_RESULT 1 passed; 0 failed; 0 ignored (normal session→Copy, shared manifest/page/status reads, per-copy uninitialized/retired containment; no applied rows, large pages or healthy idle hibernation claim)"
            ),
            Scenario::MalformedFrames => println!(
                "MALFORMED_RESULT 4 passed; 0 failed; 0 ignored (normal authenticated sockets, raw correlation/close cases, unchanged real Copy and surviving healthy session)"
            ),
            Scenario::HolderMapping => println!(
                "HOLDER_MAPPING_RESULT 1 passed; 0 failed; 0 ignored (real enrollment lookup and normal holder transport; positive unequal-ID dispatch and missing mapping with no dispatch)"
            ),
            Scenario::SessionCarryover => println!(
                "SESSION_CARRYOVER_RESULT 1 passed; 0 failed; 0 ignored (normal authenticated upgrade, exact Welcome, same-session connection replacement and physical close; no copy-path claim)"
            ),
            Scenario::InstallationDirectory => println!(
                "INSTALLATION_DIRECTORY_RESULT 3 groups passed; 0 failed; 0 ignored (shared normal-session multi-page ordering and cursor refusals, immutable revoked bindings plus submit refusal)"
            ),
            Scenario::Enrollment => println!(
                "ENROLLMENT_RESULT 2 passed; 0 failed; 0 ignored (normal HTTP; no session-admission claim)"
            ),
            Scenario::Admission => println!(
                "ADMISSION_RESULT 1 passed; 0 failed; 0 ignored (normal HTTP, real Registry publication and attachment provenance; no hibernation claim)"
            ),
            Scenario::HolderFullWait => println!(
                "HOLDER_FULL_WAIT_RESULT 1 passed; 0 failed; 0 ignored (real configured 20-second attempt, original 30-second drive lease, two calls, observed waiter counts; fixture peer answer; no full move claim)"
            ),
            Scenario::CopyWait => println!(
                "COPY_WAIT_RESULT 4 passed; 0 failed; 0 ignored (normal core Registry and Copy objects, real status/fence/retire and alarm retry; causal read/retire gap; no piece-apply, advancing-through-P or full move claim)"
            ),
            Scenario::CopyDriveOverlap => println!(
                "COPY_DRIVE_RESULT 1 passed; 0 failed; 0 ignored (sequential Release/Copy, real expired drive rejection and waiter overlap; terminal retired; no advancing-through-P or full move claim)"
            ),
            Scenario::AccessCarryover => println!(
                "ACCESS_CARRYOVER_RESULT 4 passed; 0 failed; 0 ignored (shared core HTTP verifier, real WebCrypto and fixture JWKS; zero age, key profile, verifier control and same-token expiry; no revocation claim)"
            ),
            Scenario::AccessSessions => println!(
                "ACCESS_SESSION_RESULT 1 passed; 0 failed; 0 ignored (normal shared edge, signed HTTP and sockets, real JWKS, exact binding refusal and live revocation)"
            ),
            Scenario::RevocationRace => println!(
                "REVOCATION_RESULT 2 passed; 0 failed; 0 ignored (normal revoke, held real delivery, unchanged owning enrollment, actual completed journal and late CallId replay; no heap-loss recovery claim)"
            ),
            Scenario::ReadmissionActivation => println!(
                "READMISSION_RESULT 1 passed; 0 failed; 0 ignored (real activation row and saved local receipt, normal credential/session admission, revoke before engine finish, old completion leaves row revoked and credential refused; no mailbox readback or process-crash claim)"
            ),
            Scenario::ReadmissionOpen => println!(
                "READMISSION_OPEN_RESULT 1 passed; 0 failed; 0 ignored (two real revoke/readmit cycles, delayed Open, reserved credential refusal and overlap/revoke gates, actual old Open/Close/EndSessions replays; no physical mailbox readback or process-crash claim)"
            ),
            Scenario::MailStatus => println!(
                "MAIL_STATUS_RESULT 1 passed; 0 failed; 0 ignored (normal admitted socket, own empty stream, future cursor refusal and same-socket next read; no publication or ack claim)"
            ),
            Scenario::MailSubmitAdmission => println!(
                "MAIL_SUBMIT_RESULT 3 distinct groups passed; 0 failed; 0 ignored (normal sessions, missing historical grant, enrollment pair and authenticated sender refusals; no custody readback or positive submit claim)"
            ),
            Scenario::HolderWriter => println!(
                "HOLDER_WRITER_RESULT 1 passed; 0 failed; 0 ignored (normal core route, real writer file, close/reconnect, alarm timeout and durable caller retry; explicit grant fixture; no full move-workflow or central-reset claim)"
            ),
            Scenario::Holder => println!(
                "HOLDER_RESULT 1 passed; 0 failed; 0 ignored (normal HTTP, distinct holder lookup and separate-event release/withdrawal answers; no writer-idempotency or hibernation claim)"
            ),
            Scenario::Renewal => println!(
                "LEASE_RESULT 1 passed; 0 failed; 0 ignored (normal HTTP, real Registry renewal; no positive registration or holder-change claim)"
            ),
        }
    }
}
