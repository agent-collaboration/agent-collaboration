//! Where the runtime reports what it observed; the host decides how to record it.

pub trait Observer<Observation>: Send + Sync {
    fn observe(&self, observation: Observation);
}

impl<Observation, Report: Fn(Observation) + Send + Sync> Observer<Observation> for Report {
    fn observe(&self, observation: Observation) {
        self(observation);
    }
}
