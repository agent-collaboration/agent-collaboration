WITH pending AS (SELECT * FROM changes WHERE position > ?1), selected_threads AS (SELECT DISTINCT thread_id FROM pending WHERE thread_id IS NOT NULL)
SELECT * FROM (
SELECT 'change' AS row_kind,json_object('changeId',c.change_id,'position',c.position,'item',json_object('kind',c.item_kind,'id',c.item_id),'actor',c.actor,'changeKind',c.change_kind,'threadId',c.thread_id,'projectId',c.project_id,'subjectActor',c.subject_actor,'role',c.role,'createdAt',c.created_at) AS payload FROM pending c
UNION ALL
SELECT 'thread',json_object('threadId',t.thread_id,'areaId',t.area_id,'title',t.title,'creator',t.creator,'state',t.state) FROM threads t WHERE t.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'member',json_object('threadMemberId',m.thread_member_id,'threadId',m.thread_id,'actorIdentityId',m.actor_identity_id,'role',m.member_role,'notifications',CASE m.notifications WHEN 1 THEN 'on' WHEN 0 THEN 'off' ELSE NULL END,'state',json(CASE WHEN m.left_at IS NULL THEN '{"kind":"joined"}' ELSE json_object('kind','left','at',m.left_at) END)) FROM thread_members m WHERE m.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'post',json_object('postId',p.post_id,'threadId',p.thread_id,'author',p.author,'body',p.body,'addressees',p.addressees) FROM posts p JOIN pending c ON c.item_kind='post' AND c.item_id=p.post_id
)
UNION ALL
SELECT * FROM (
SELECT 'projectRole',json_object('projectRoleId',r.project_role_id,'projectId',r.project_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) FROM project_roles r
UNION ALL
SELECT 'deliverableRole',json_object('deliverableRoleId',r.deliverable_role_id,'deliverableId',r.deliverable_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) FROM deliverable_roles r
UNION ALL
SELECT 'highWater',CAST(COALESCE(MAX(position),0) AS TEXT) FROM changes
);
