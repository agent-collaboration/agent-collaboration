SELECT CAST(json_object(
'automationId',a.automation_id,'kind',a.kind,'state',a.state,'revision',a.revision,
'definitionJson',a.definition_json,'creatorJson',a.creator_json,'legacyTimingJson',a.legacy_timing_json,
'slotSessionKey',a.slot_session_key,'anchorAtMs',a.anchor_at_ms,'createdAtMs',a.created_at_ms,
'updatedAtMs',a.updated_at_ms,'executionMode',a.execution_mode,'importedContinuityJson',a.imported_continuity_json,
'faultJson',a.fault_json,'faultedAtMs',a.faulted_at_ms,'nextDueMs',t.next_due_ms,'expiresAtMs',t.expires_at_ms,
'admittedThroughMs',t.admitted_through_ms,'importedWatermarkLocalMs',t.imported_watermark_local_ms,
'occupyingFiringId',t.occupying_firing_id,'waitingFiringId',t.waiting_firing_id,'externalRunId',t.occupying_external_run,
'imports',json((SELECT json_group_array(json_object('importId',r.import_id,'serviceId',r.service_id,'automationId',r.automation_id,'digest',r.digest)) FROM (SELECT * FROM automation_imports ORDER BY import_id) r)),
'contentRevisions',json((SELECT json_group_array(json_object('contentId',c.content_id,'ownerId',c.owner_id,'revisionId',c.revision_id,'recordedAtMs',c.recorded_at_ms,'digest',c.digest,'text',c.text)) FROM (SELECT * FROM content_revisions ORDER BY content_id) c)),
'instructionDocuments',json((SELECT json_group_array(json_object('instructionId',d.instruction_id,'currentContentId',d.current_content_id,'createdAtMs',d.created_at_ms,'updatedAtMs',d.updated_at_ms)) FROM (SELECT * FROM instruction_documents ORDER BY instruction_id) d)),
'counts',json_object('automations',(SELECT COUNT(*) FROM automations),'timing',(SELECT COUNT(*) FROM automation_timing),'firings',(SELECT COUNT(*) FROM automation_firings),'batches',(SELECT COUNT(*) FROM automation_batches),'imports',(SELECT COUNT(*) FROM automation_imports),'contentRevisions',(SELECT COUNT(*) FROM content_revisions),'instructionDocuments',(SELECT COUNT(*) FROM instruction_documents)),
'batches',json((SELECT json_group_array(json_object('batchId',b.batch_id,'serviceId',b.service_id,'digest',b.digest,'frozenBytes',b.frozen_bytes,'ackedAtMs',b.acked_at_ms)) FROM (SELECT * FROM automation_batches ORDER BY batch_id) b)),
'firings',json((SELECT json_group_array(json_object('firingId',f.firing_id,'dueMs',f.due_ms,'foldedThroughMs',f.folded_through_ms,'pendingEntryJson',f.pending_entry_json,'state',f.state,'batchId',f.batch_id,'reportSeq',f.report_seq,'outcomeJson',f.outcome_json,'reportServiceId',f.report_service_id)) FROM (SELECT * FROM automation_firings WHERE automation_id=a.automation_id ORDER BY firing_id) f))
) AS TEXT) AS observation
FROM (SELECT ?1 AS automation_id) requested
LEFT JOIN automations a ON a.automation_id=requested.automation_id
LEFT JOIN automation_timing t ON t.automation_id=a.automation_id;
