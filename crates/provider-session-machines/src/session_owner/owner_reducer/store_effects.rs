use super::publication_effects::LOCAL_BUDGET;
use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_owner::{
    EffectOutcome, OwnerDeadline, OwnerEffect, SessionOwnerReply, SessionStoreRequest,
    TerminalWriteId,
};

impl OwnerTransition {
    pub(super) fn record_terminal(&mut self, reply: SessionOwnerReply) {
        let Some(sequence) = self.next.ports.terminal_sequence.checked_add(1) else {
            self.effects.push(OwnerEffect::ReplyUnsaved(reply));
            return;
        };
        self.next.ports.terminal_sequence = sequence;
        let write = TerminalWriteId {
            incarnation: self.next.ports.address.incarnation,
            sequence,
        };
        self.next
            .ports
            .pending_terminal
            .insert(write, reply.clone());
        self.effects
            .push(OwnerEffect::Store(SessionStoreRequest::RecordTerminal {
                write,
                reply: Box::new(reply),
            }));
        self.arm(OwnerDeadline::TerminalStore(write), LOCAL_BUDGET);
    }

    pub(super) fn terminal_recorded(
        &mut self,
        write: TerminalWriteId,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let reply = self
            .next
            .ports
            .pending_terminal
            .remove(&write)
            .ok_or(OwnerRejection::IgnoredLate)?;
        self.effects.push(if outcome == EffectOutcome::Applied {
            OwnerEffect::Reply(reply)
        } else {
            // Accepted lane work keeps its custody. No extra read is needed
            // to return the already observed result to the caller.
            OwnerEffect::ReplyUnsaved(reply)
        });
        Ok(())
    }
}
