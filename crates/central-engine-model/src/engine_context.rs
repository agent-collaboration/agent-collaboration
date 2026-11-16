pub type EngineTime = chrono::DateTime<chrono::Utc>;

pub trait IdSource {
    fn next_id(&mut self) -> uuid::Uuid;
}

pub struct Context<'a> {
    pub now: EngineTime,
    pub ids: &'a mut dyn IdSource,
}
