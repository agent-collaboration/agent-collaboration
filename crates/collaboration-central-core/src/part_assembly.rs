use collaboration_replication_protocol::{Piece, PieceDigest, ReplicatedTransaction};

pub(crate) fn assemble_transaction(
    parts: &[Piece],
    incoming: &Piece,
) -> Option<ReplicatedTransaction> {
    let descriptor = incoming.part.as_ref()?;
    if parts.len() != usize::try_from(descriptor.count).ok()? {
        return None;
    }
    let mut transaction = None;
    for (index, piece) in parts.iter().enumerate() {
        let part = piece.part.as_ref()?;
        if usize::try_from(part.index).ok()? != index
            || part.count != descriptor.count
            || part.transaction != descriptor.transaction
            || part.transaction_digest != descriptor.transaction_digest
            || piece.project != incoming.project
            || piece.epoch != incoming.epoch
            || piece.continues_from != incoming.continues_from
            || piece.through != incoming.through
        {
            return None;
        }
        let fragment = piece.transactions.first()?;
        if piece.transactions.len() != 1 || fragment.commit_id() != descriptor.transaction.commit_id
        {
            return None;
        }
        match (&mut transaction, fragment) {
            (None, fragment) => transaction = Some(fragment.clone()),
            (
                Some(ReplicatedTransaction::Rows { rows, .. }),
                ReplicatedTransaction::Rows { rows: more, .. },
            ) => rows.extend(more.iter().cloned()),
            (
                Some(ReplicatedTransaction::SchemaStep {
                    migration_version,
                    statements,
                    ..
                }),
                ReplicatedTransaction::SchemaStep {
                    migration_version: more_version,
                    statements: more,
                    ..
                },
            ) if migration_version == more_version => statements.extend(more.iter().cloned()),
            _ => return None,
        }
    }
    let transaction = transaction?;
    (PieceDigest::of_transaction(&transaction).ok()? == descriptor.transaction_digest)
        .then_some(transaction)
}
