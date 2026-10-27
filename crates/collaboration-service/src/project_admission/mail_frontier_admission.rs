//! Only a publisher's unchanged, terminal prefix can advance the replicated frontier.
use super::{ProjectAdmission, ProjectAdmissionError};
use crate::notification_publisher::SettledMailPrefix;

impl ProjectAdmission {
    pub(crate) async fn advance_mail_frontier(
        &self,
        observed: SettledMailPrefix,
    ) -> Result<bool, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(observed.project()).await? else {
            return Ok(false);
        };
        let mut write = store.begin_admission(self.record_origin()).await?;
        if !observed.still_current(&mut write).await? {
            write.rollback().await?;
            return Ok(false);
        }
        if !write
            .advance_mail_settled_through(observed.through())
            .await?
        {
            write.rollback().await?;
            return Ok(false);
        }
        write.commit().await?;
        self.registry.notify_activity(observed.project());
        Ok(true)
    }
}
