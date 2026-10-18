WITH same_reader AS (
    SELECT reader, scope, membership_kind, membership_id, thread_id
    FROM thread_subscriptions
    WHERE reader = ? AND scope > ? ORDER BY scope LIMIT 101
), later_readers AS (
    SELECT reader, scope, membership_kind, membership_id, thread_id
    FROM thread_subscriptions
    WHERE reader > ? ORDER BY reader, scope LIMIT 101
)
SELECT reader AS "reader!: String", scope AS "scope!: String", membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM same_reader
UNION ALL
SELECT reader AS "reader!: String", scope AS "scope!: String", membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM later_readers
ORDER BY 1, 2 LIMIT 101
