UPDATE automation_timing SET expires_at_ms=json_extract(?1,'$.expiresAtMs') WHERE automation_id=json_extract(?1,'$.automationId');
