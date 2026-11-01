UPDATE automations SET anchor_at_ms=CAST(? AS INTEGER),imported_continuity_json=? WHERE automation_id=?;
