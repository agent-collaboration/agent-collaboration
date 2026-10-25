//! One session owner acknowledges only its received, successfully handled prefix.
use fleet_sync_protocol::NoticePosition;
use std::collections::{BTreeMap, btree_map::Entry};

enum NoticeReceipt {
    Pending,
    Handled,
    Failed,
}

#[derive(Default)]
pub(super) struct NoticeReceipts {
    received: BTreeMap<NoticePosition, NoticeReceipt>,
    acknowledged: Option<NoticePosition>,
}

impl NoticeReceipts {
    pub fn begin(&mut self, position: NoticePosition) -> bool {
        if self
            .acknowledged
            .is_some_and(|acknowledged| position <= acknowledged)
        {
            return false;
        }
        match self.received.entry(position) {
            Entry::Vacant(receipt) => {
                receipt.insert(NoticeReceipt::Pending);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn complete(&mut self, position: NoticePosition, handled: bool) {
        if let Some(receipt @ NoticeReceipt::Pending) = self.received.get_mut(&position) {
            *receipt = if handled {
                NoticeReceipt::Handled
            } else {
                NoticeReceipt::Failed
            };
        }
    }

    pub fn handled_prefix(&self) -> Option<NoticePosition> {
        self.received
            .iter()
            .take_while(|(_, receipt)| matches!(receipt, NoticeReceipt::Handled))
            .map(|(position, _)| *position)
            .last()
    }

    pub fn acknowledged(&mut self, position: NoticePosition) {
        self.received.retain(|received, _| *received > position);
        self.acknowledged = Some(position);
    }

    pub fn acknowledgement_failed(&mut self) {
        // Preserve today's fail-closed rule until the next connection. A
        // possibly written acknowledgement never permits skipping a receipt.
        if let Some(receipt) = self.received.first_entry().map(|entry| entry.into_mut()) {
            *receipt = NoticeReceipt::Failed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_order_cannot_acknowledge_past_an_earlier_pending_receipt() {
        let mut receipts = NoticeReceipts::default();
        let first = NoticePosition::new(10);
        let later = NoticePosition::new(20);
        assert!(receipts.begin(first));
        assert!(receipts.begin(later));
        receipts.complete(later, true);
        assert_eq!(receipts.handled_prefix(), None);
        receipts.complete(first, true);
        assert_eq!(receipts.handled_prefix(), Some(later));
        receipts.acknowledged(later);
        assert!(!receipts.begin(first));
        assert!(!receipts.begin(later));
        assert_eq!(receipts.handled_prefix(), None);
    }

    #[test]
    fn failed_handling_blocks_later_success_until_connection_reset() {
        let mut receipts = NoticeReceipts::default();
        let first = NoticePosition::new(1);
        let failed = NoticePosition::new(2);
        let later = NoticePosition::new(3);
        assert!(receipts.begin(first));
        assert!(receipts.begin(failed));
        assert!(receipts.begin(later));
        receipts.complete(first, true);
        receipts.complete(failed, false);
        receipts.complete(later, true);
        assert_eq!(receipts.handled_prefix(), Some(first));
        receipts.acknowledged(first);
        assert_eq!(receipts.handled_prefix(), None);
        assert!(!receipts.begin(failed));
        receipts.complete(failed, true);
        assert_eq!(receipts.handled_prefix(), None);
    }

    #[test]
    fn an_acknowledgement_error_cannot_be_retried_by_a_later_completion() {
        let mut receipts = NoticeReceipts::default();
        let first = NoticePosition::new(1);
        let later = NoticePosition::new(2);
        assert!(receipts.begin(first));
        assert!(receipts.begin(later));
        receipts.complete(first, true);
        receipts.acknowledgement_failed();
        receipts.complete(later, true);
        assert_eq!(receipts.handled_prefix(), None);
    }
}
