UPDATE automation_timing SET occupying_external_run=json_extract(?1,'$.runId') WHERE automation_id=json_extract(?1,'$.automationId') AND occupying_firing_id IS NULL AND waiting_firing_id IS NULL;
