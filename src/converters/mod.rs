use crate::pb::common::base::Base;
use crate::pb::service::sharing::{Expense, ExpenseLeg, SplitMethod};

// ---------------------------------------------------------------------------
// DB row structs
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
pub struct DbExpense {
    pub id: String,
    pub budget_id: String,
    pub paid_by: String,
    pub total_amount: i64,
    pub description: String,
    pub expense_date: String,
    pub category_id: Option<String>,
    pub split_method: String,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DbExpenseLeg {
    pub id: String,
    pub expense_id: String,
    pub user_id: String,
    pub amount: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DbBalance {
    pub user_id: String,
    pub net_balance: i64,
}

// ---------------------------------------------------------------------------
// String ↔ Enum helpers
// ---------------------------------------------------------------------------

pub fn split_method_to_db(m: SplitMethod) -> &'static str {
    match m {
        SplitMethod::Equal => "equal",
        SplitMethod::Custom => "custom",
        SplitMethod::Weighted => "weighted",
        SplitMethod::Unspecified => "equal",
    }
}

pub fn split_method_from_db(s: &str) -> SplitMethod {
    match s {
        "custom" => SplitMethod::Custom,
        "weighted" => SplitMethod::Weighted,
        _ => SplitMethod::Equal,
    }
}

// ---------------------------------------------------------------------------
// DB row → Proto
// ---------------------------------------------------------------------------

pub fn map_expense(db: DbExpense, legs: Vec<DbExpenseLeg>) -> Expense {
    Expense {
        base: Some(Base {
            id: db.id,
            created_by: db.created_by,
            created_at: db.created_at,
            updated_at: db.updated_at,
            deleted_at: 0,
            updated_by: String::new(),
            owner_id: String::new(),
            status: 0,
        }),
        budget_id: db.budget_id,
        paid_by: db.paid_by,
        total_amount: db.total_amount,
        description: db.description,
        expense_date: db.expense_date,
        category_id: db.category_id.unwrap_or_default(),
        split_method: split_method_from_db(&db.split_method) as i32,
        legs: legs
            .into_iter()
            .map(|l| ExpenseLeg {
                user_id: l.user_id,
                amount: l.amount,
            })
            .collect(),
    }
}
