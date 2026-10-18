use crate::object_context::ObjectContext;
use collaboration_central_conformance::StorageCorruption;
use worker::*;
pub(crate) async fn corrupt(context: &ObjectContext, mutation: StorageCorruption) -> Result<()> {
    let statements = match mutation {
        StorageCorruption::RestoreUnheldRegistry { unheld, as_of } => {
            let project = SqlStorageValue::String(unheld.project.as_str().into());
            vec![
                (
                    "DELETE FROM _registry_leases WHERE project=?",
                    vec![project.clone()],
                ),
                (
                    "DELETE FROM _registry_transfers WHERE project=?",
                    vec![project.clone()],
                ),
                (
                    "DELETE FROM _registry_history WHERE project=?",
                    vec![project.clone()],
                ),
                (
                    "DELETE FROM _registry_claim_rejections WHERE project=?",
                    vec![project.clone()],
                ),
                (
                    "INSERT INTO _registry_epoch_floor(project,epoch) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET epoch=excluded.epoch",
                    vec![
                        project.clone(),
                        SqlStorageValue::String(unheld.import_epoch.value().to_string()),
                    ],
                ),
                (
                    "INSERT INTO _registry_unheld(project,record) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET record=excluded.record",
                    vec![
                        project,
                        SqlStorageValue::String(serde_json::to_string(
                            &serde_json::json!({"unheld":unheld,"change":1,"asOf":as_of}),
                        )?),
                    ],
                ),
            ]
        }
        StorageCorruption::RemovePieceReceipts => {
            vec![("DELETE FROM _copy_receipts", Vec::new())]
        }
        StorageCorruption::ArchiveCopy => {
            vec![("UPDATE _copy_state SET archived=1", Vec::new())]
        }
        StorageCorruption::RemoveCopyState => vec![("DELETE FROM _copy_state", Vec::new())],
        StorageCorruption::InvalidateCopyFence => {
            vec![("UPDATE _copy_state SET fence_epoch='invalid'", Vec::new())]
        }
        StorageCorruption::ForgetRegistryFloor { project } => vec![(
            "DELETE FROM _registry_epoch_floor WHERE project=?",
            vec![SqlStorageValue::String(project.as_str().into())],
        )],
    };
    let store = context.storage.sql();
    context
        .storage
        .transaction(move |_transaction| async move {
            for (sql, parameters) in statements {
                store
                    .exec(sql, Some(parameters))?
                    .to_array::<serde::de::IgnoredAny>()?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}
