CREATE INDEX tasks_by_workstream ON tasks(workstream_id, task_id);
CREATE INDEX trail_by_tag ON work_trail_tags(tag, row_id);
CREATE INDEX feed_by_record ON project_feed(record_kind, record_id, position);
