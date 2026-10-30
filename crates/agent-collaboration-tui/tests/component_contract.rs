#[cfg(test)]
#[path = "suites/harness_contract.rs"]
mod harness_contract;
#[cfg(test)]
#[path = "suites/hit_geometry.rs"]
mod hit_geometry;
#[cfg(test)]
#[path = "suites/input_isolation.rs"]
mod input_isolation;
#[cfg(test)]
mod support;
#[cfg(test)]
#[path = "suites/surface_exceptions.rs"]
mod surface_exceptions;
#[cfg(test)]
#[path = "suites/wheel_isolation.rs"]
mod wheel_isolation;
