//! Only page-selected immutable records are decoded into inbox activities.
use message_board::*;
pub(super) fn inbox_activity(entry: FeedEntry) -> Result<InboxActivity, BoardError> {
    let cursor = FeedCursor {
        epoch: entry.epoch,
        position: entry.position,
    };
    let FeedContent::Shown(record) = entry.content else {
        return Err(BoardError::board_unavailable());
    };
    match *record {
        Record::Post(post) => {
            let scope = match &post.placement {
                Placement::Project { project_id } => ReadScope::Project {
                    project_id: project_id.clone(),
                },
                Placement::Thread { thread_id } => ReadScope::Thread {
                    thread_id: thread_id.clone(),
                },
            };
            Ok(InboxActivity::PostCreated {
                workspace_id: post.envelope.scope.workspace.clone(),
                position: post.position,
                acknowledgement_scope: scope,
                post: Box::new(post),
            })
        }
        Record::Thread(thread) => Ok(InboxActivity::ThreadStateChanged {
            workspace_id: thread.envelope.scope.workspace,
            board_id: thread
                .envelope
                .scope
                .board
                .ok_or_else(BoardError::board_unavailable)?,
            position: RecordPosition::Project(cursor),
            acknowledgement_scope: ReadScope::Thread {
                thread_id: thread.thread_id.clone(),
            },
            thread_id: thread.thread_id,
            project_id: thread
                .envelope
                .scope
                .project
                .ok_or_else(BoardError::board_unavailable)?,
            actor: thread
                .envelope
                .actor
                .declared_identity()
                .ok_or_else(BoardError::board_unavailable)?
                .clone(),
            state: thread.state,
        }),
        _ => Err(BoardError::board_unavailable()),
    }
}
