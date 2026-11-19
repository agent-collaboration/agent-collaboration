//! Per-scenario resources and literal board inputs shared by the process journeys.

pub fn identity() -> String {
    uuid::Uuid::now_v7().to_string()
}
