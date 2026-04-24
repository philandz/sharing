#![allow(clippy::result_large_err)]

use tonic::{metadata::MetadataMap, Status};

pub fn user_id_from_metadata(meta: &MetadataMap) -> Result<String, Status> {
    meta.get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| Status::unauthenticated("Missing x-user-id metadata"))
}

pub fn non_empty(field: &str, value: &str) -> Result<(), Status> {
    if value.trim().is_empty() {
        Err(Status::invalid_argument(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

pub fn positive_amount(amount: i64) -> Result<(), Status> {
    if amount <= 0 {
        Err(Status::invalid_argument("amount must be positive"))
    } else {
        Ok(())
    }
}
