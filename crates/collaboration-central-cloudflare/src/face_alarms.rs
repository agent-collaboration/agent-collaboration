//! One physical timer, with face ownership derived from the core's durable deadlines.
use crate::object_context::ObjectContext;
use collaboration_central_core::*;
impl ObjectContext {
    pub(crate) async fn set_face_alarm(
        &self,
        face: CallerKind,
        _at: RegistryTime,
    ) -> worker::Result<()> {
        if face != self.face {
            return Err(worker::Error::RustError(
                "alarm face differs from context".into(),
            ));
        }
        self.refresh_alarm().await
    }
    pub(crate) async fn refresh_alarm(&self) -> worker::Result<()> {
        let plan = crate::cloudflare_storage::alarm_plan(&self.storage, &self.address)?;
        match plan.iter().map(|alarm| alarm.at).min_by_key(|at| at.0) {
            Some(at) => self.storage.set_alarm(at.0).await,
            None => self.storage.delete_alarm().await,
        }
    }
    pub(crate) async fn take_due_alarms(&self) -> worker::Result<Vec<CallerKind>> {
        let now = self.now().await?;
        let plan = crate::cloudflare_storage::alarm_plan(&self.storage, &self.address)?;
        Ok(plan
            .into_iter()
            .filter(|alarm| alarm.at.0.timestamp_millis() <= now.0.timestamp_millis())
            .map(|alarm| alarm.face)
            .collect())
    }
}
