#![allow(clippy::result_large_err)]

use tonic::{metadata::MetadataMap, Status};

/// Read the JWT-authenticated user id from the `x-user-id` metadata
/// header (set by the gateway after validating the bearer token).
pub fn user_id_from_metadata(meta: &MetadataMap) -> Result<String, Status> {
    meta.get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| Status::unauthenticated("Missing x-user-id metadata"))
}

/// Read the guest session token from the `x-session-token` metadata
/// header (set by the gateway when `Authorization: SharingSession …`
/// is sent). Returns None if the header is absent or empty.
pub fn session_token_from_metadata(meta: &MetadataMap) -> Option<String> {
    meta.get("x-session-token")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Read the budget_id from the `x-budget-id` metadata header. The
/// gateway injects this from the path so the handler can pass it to
/// the biz layer for participant lookups (avoids trusting the
/// `budget_id` field in the request body for guest auth).
pub fn budget_id_from_metadata(meta: &MetadataMap) -> Option<String> {
    meta.get("x-budget-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
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
