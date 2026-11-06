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

/// Longest list window a client may ask for.
pub const LIST_MAX_RANGE_DAYS: i64 = 366;
pub const LIST_MAX_PAGE_SIZE: i32 = 100;
const LIST_MAX_IDS: usize = 50;

/// Build the repository filter from a ListExpenses request, rejecting bad
/// dates, inverted/oversized ranges and oversized id lists.
pub fn expense_filter(
    req: &crate::pb::service::sharing::ListExpensesRequest,
) -> Result<crate::manager::repository::ExpenseFilter, Status> {
    use crate::manager::repository::{ExpenseFilter, ExpenseScope, ExpenseSort};

    fn parse(field: &str, v: &Option<String>) -> Result<Option<chrono::NaiveDate>, Status> {
        match v.as_deref().filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| Status::invalid_argument(format!("{field} must be YYYY-MM-DD"))),
        }
    }
    let from = parse("date_from", &req.date_from)?;
    let to = parse("date_to", &req.date_to)?;
    if let (Some(f), Some(t)) = (from, to) {
        if f > t {
            return Err(Status::invalid_argument(
                "date_from must not be after date_to",
            ));
        }
        if (t - f).num_days() + 1 > LIST_MAX_RANGE_DAYS {
            return Err(Status::invalid_argument(format!(
                "date range must not exceed {LIST_MAX_RANGE_DAYS} days"
            )));
        }
    }
    for (name, ids) in [
        ("paid_by_ids", &req.paid_by_ids),
        ("share_participant_ids", &req.share_participant_ids),
        ("category_ids", &req.category_ids),
    ] {
        if ids.len() > LIST_MAX_IDS {
            return Err(Status::invalid_argument(format!(
                "{name} accepts at most {LIST_MAX_IDS} ids"
            )));
        }
    }
    Ok(ExpenseFilter {
        q: req.q.clone(),
        date_from: from.map(|d| d.format("%Y-%m-%d").to_string()),
        date_to: to.map(|d| d.format("%Y-%m-%d").to_string()),
        paid_by_ids: req.paid_by_ids.clone(),
        share_participant_ids: req.share_participant_ids.clone(),
        category_ids: req.category_ids.clone(),
        scope: match req.scope.as_deref() {
            Some("shared") => ExpenseScope::Shared,
            Some("personal") => ExpenseScope::Personal,
            _ => ExpenseScope::All,
        },
        sort_by: if req.sort_by.as_deref() == Some("amount") {
            ExpenseSort::Amount
        } else {
            ExpenseSort::Date
        },
        ascending: req.sort_dir.as_deref() == Some("asc"),
        page: req.page.unwrap_or(1).max(1),
        page_size: req.page_size.map_or(0, |n| n.clamp(0, LIST_MAX_PAGE_SIZE)),
    })
}

#[cfg(test)]
mod expense_filter_tests {
    use super::*;
    use crate::pb::service::sharing::ListExpensesRequest;

    fn req() -> ListExpensesRequest {
        ListExpensesRequest {
            budget_id: "b".into(),
            ..Default::default()
        }
    }

    #[test]
    fn bare_request_means_everything_unpaged() {
        let f = expense_filter(&req()).unwrap();
        assert_eq!(f.page_size, 0);
        assert!(f.date_from.is_none() && f.paid_by_ids.is_empty());
        assert!(!f.ascending);
    }

    #[test]
    fn a_year_is_allowed_and_more_is_refused() {
        let mut r = req();
        r.date_from = Some("2026-01-01".into());
        r.date_to = Some("2026-12-31".into());
        assert!(expense_filter(&r).is_ok());
        r.date_to = Some("2027-01-02".into());
        assert!(expense_filter(&r).is_err());
    }

    #[test]
    fn bad_and_inverted_dates_are_refused() {
        let mut r = req();
        r.date_from = Some("01/02/2026".into());
        assert!(expense_filter(&r).is_err());
        r.date_from = Some("2026-02-02".into());
        r.date_to = Some("2026-02-01".into());
        assert!(expense_filter(&r).is_err());
    }

    #[test]
    fn scope_maps_and_unknown_means_all() {
        use crate::manager::repository::ExpenseScope;
        let mut r = req();
        r.scope = Some("shared".into());
        assert_eq!(expense_filter(&r).unwrap().scope, ExpenseScope::Shared);
        r.scope = Some("personal".into());
        assert_eq!(expense_filter(&r).unwrap().scope, ExpenseScope::Personal);
        r.scope = Some("whatever".into());
        assert_eq!(expense_filter(&r).unwrap().scope, ExpenseScope::All);
    }

    #[test]
    fn page_size_is_clamped_and_ids_capped() {
        let mut r = req();
        r.page_size = Some(10_000);
        assert_eq!(expense_filter(&r).unwrap().page_size, LIST_MAX_PAGE_SIZE);
        r.paid_by_ids = (0..51).map(|i| i.to_string()).collect();
        assert!(expense_filter(&r).is_err());
    }
}
