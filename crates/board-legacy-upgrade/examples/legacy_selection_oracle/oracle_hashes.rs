use crate::oracle_errors::OracleError;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

pub(crate) fn sha256_file(path: &Path) -> Result<String, OracleError> {
    let contents = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(contents)))
}

pub(crate) fn sha256_post_set(post_ids: &[String]) -> Result<String, OracleError> {
    let unique_post_ids: BTreeSet<&str> = post_ids.iter().map(String::as_str).collect();
    let sorted_unique_post_ids: Vec<&str> = unique_post_ids.into_iter().collect();
    let canonical_json = serde_json::to_vec(&sorted_unique_post_ids)?;
    Ok(format!("{:x}", Sha256::digest(canonical_json)))
}
