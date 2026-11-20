//! One namespace per original central runtime test root.

#[cfg(all(feature = "test-support", unix))]
mod organization_signin;

#[cfg(test)]
#[cfg(feature = "test-support")]
mod alarm_publication_order;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod central_object_batches;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod request_pause_seam;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod storage_support;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod workspace_store_calls;

#[cfg(test)]
#[cfg(feature = "test-support")]
mod workspace_requests;

#[cfg(feature = "test-support")]
mod workspace_link;
