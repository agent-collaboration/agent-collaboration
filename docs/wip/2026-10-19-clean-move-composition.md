# Clean move composition frontier

Source: central258fefc599a1204057933b5b5a5c2140dfa47138, library9331f5e.
Commission: Lead01a15326,01a15343 and01a15347. Source-only while Verifier B
holds the build/runtime target. This is implementation preparation, not a
passing holder-change result or a new governing design.

## Owning paths read

- central_router currently has enrollment, revocation and session routes, but
  no normal move route. The existing ConformanceClient::admin expects
  `/admin/v1/projects/{project}/move` and today's tagged AdminRequest::Move.
- Registry's object transport currently admits only enrollment/revocation and
  authentication through its legacy edge-call family. Owner authority must
  come from Access/CallAuthority::Owner; the body's AdminAuthority is no grant.
- shared_task_registry already registers the real HolderChangeTask. Registry's
  on_alarm drives that Engine; CoreHost resolves holder PeerId through the
  committed enrollment directory, then calls the real Delivery face.
- HolderRelease uses the reply hub and the exact attached session, outside local
  exclusion. HolderChangeTask saves release intent, awaits the real Copy's
  acknowledged final position, fences, and only then grants. CopyAcknowledged,
  fence and holder transport are already composed; no parallel workflow is added.
- The existing grant setup fixture uses real RegistrationTask and
  HolderChangeTask, but substitutes an idle Released(1:0) holder. It proves
  setup history for C25; it does not prove the new normal clean-move path.

## Library reason gap and settlement

Spec3 E27 requires requester and reason retained for the project's life, and
same-ID/different-content refusal. At9331f5e HolderChangeRequest::Move carries
only attempt/database/from_epoch/to. admit_holder_change accepts the request,
ActorRef and time. TransferAdmission, TransferIntent and HolderChangeView have
no reason. One-core assigns the transfer record to the library.

Lead01a15347 routes this to W-C34/C32: Move and Takeover gain bounded reason
as request content, stored, conflict-checked and viewed for record life. No
central copy. Until pinned, today's AdminRequest::Move{reason} stays unchanged;
the bounded type/conversion and composed runtime wiring remain pending. A reason
is never hidden in ActorRef or a duplicate central transfer row.

The source-only AdmitMove callback draft is in the session scratch's
2026-10-clean-move-composition/move_admission.rs. It accepts an already validated
library Move request, so C32's reason remains in that request; it does not guess
the future constructor. It checks saved request content through public
holder_change_view, admits with public admit_holder_change in the one step
transaction, reads canonical outcome or Pending, and returns postcommit wake
activation. It is unregistered and uncompiled. HTTP/request conversion is not
claimed implemented by that draft.

## Replay binding that the adapter must preserve

Today's admin Move body has no from_epoch. For a first attempt the Registry
reads its own lease and binds that epoch. For a retry after grant it must use
the original request exposed by holder_change_view; re-deriving from_epoch
from today's lease would turn an identical replay into a conflict. Verify the
saved project, target and C32 reason against the offered body, then reuse that
saved request. Preserve the original requester metadata. No current lease or
new admission is allowed to overwrite the old attempt's meaning.

## Permanent real-path scenario to build after the pin

1. Enroll two distinct service/peer bindings and connect both real websocket
   sessions with completed Hello/schema publication. Create an actual epoch1
   project/Copy through public registration setup; setup remains labelled.
2. Send today's AdminRequest::Move from a machine credential and require
   adminRequired/access refusal, with exact lease/history/Copy unchanged.
3. Send the owner move with a fixed attempt and reason. Capture the real
   old-holder Release request and exact frame/transfer/database/epoch. Answer
   from that socket with a final captured position ahead of Copy's ack.
4. Observe public holder_change_view in Released waiting on Copy. The target
   gets no grant and Copy has not fenced. Advance Copy through the holder's
   actual replication path; no sleep or direct state fabrication substitutes.
5. Observe one fence and one epoch2 grant starting at that exact final position.
   Restart the actual server/stores; repeat identical admin attempt, require the
   original outcome/new epoch/start, and independently read physical state.
6. Repeat the same attempt with changed reason or target and require
   conflictingRequest, with no epoch3/fence/lease mutation. Read requester and
   original reason through the public C32 record/view after reopen.

The permanent scenario must fail with the normal move route missing, the
saved-request replay binding bypassed, early fencing/grant before ack, or reason
dropped. No pre-fix run has been observed for this draft. A normal socket holder
answer is the central transport proof; real Router freeze/capture/seed/admission
is A/G3, and is not inferred from a fixture's Release answer.

Directory lease publication still awaits C30. Claim/archive remain pending
their named gates; takeover/cutoff/lineage wait their adopted carriers/pins.
No legacy test or implementation is removed by this preparation.
