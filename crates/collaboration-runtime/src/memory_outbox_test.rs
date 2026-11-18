/// The shared OutboxStorage contract on the in-memory store.
mod outbox_storage_contract {
    fn open() -> std::future::Ready<((), crate::MemoryOutbox)> {
        std::future::ready(((), crate::MemoryOutbox::new()))
    }
    crate::outbox_storage_contract::outbox_storage_contract!(open());
}
