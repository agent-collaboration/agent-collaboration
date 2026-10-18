CREATE INDEX feed_by_kind_position ON project_feed(record_kind, position);
CREATE INDEX inbox_feed_by_position ON project_feed(position) WHERE record_kind IN ('post', 'thread');
