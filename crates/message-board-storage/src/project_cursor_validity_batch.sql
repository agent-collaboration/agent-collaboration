WITH cursors AS (
 SELECT CAST(json_extract(value,'$[0]') AS INTEGER) AS cursor_epoch,
        CAST(json_extract(value,'$[1]') AS INTEGER) AS cursor_position FROM json_each(?)
), facts AS (
 SELECT c.cursor_epoch,c.cursor_position,
  (SELECT COUNT(*) FROM project_feed f WHERE f.position=c.cursor_position) AS retained_count,
  (SELECT MIN(f.epoch) FROM project_feed f WHERE f.position=c.cursor_position) AS retained_epoch,
  k.line_id AS known_line_id,
  n.line_id AS later_line_id,n.epoch AS later_epoch,n.inherited_through AS inherited_through
 FROM cursors c
 LEFT JOIN epoch_lines k ON k.epoch=c.cursor_epoch
 LEFT JOIN epoch_lines n ON n.epoch=(SELECT MIN(next.epoch) FROM epoch_lines next WHERE next.epoch>c.cursor_epoch)
)
SELECT cursor_epoch AS "cursor_epoch!: i64",cursor_position AS "cursor_position!: i64",
 retained_count AS "retained_count!: i64",retained_epoch AS "retained_epoch?: i64",
 known_line_id AS "known_line_id?: String",later_line_id AS "later_line_id?: String",
 later_epoch AS "later_epoch?: i64",inherited_through AS "inherited_through?: i64",
 (SELECT COUNT(*) FROM project_feed r WHERE r.position=facts.inherited_through) AS "resume_count!: i64",
 (SELECT MIN(r.epoch) FROM project_feed r WHERE r.position=facts.inherited_through) AS "resume_epoch?: i64",
 (SELECT MIN(r.entry_id) FROM project_feed r WHERE r.position=facts.inherited_through) AS "resume_entry_id?: String"
FROM facts ORDER BY cursor_epoch,cursor_position
