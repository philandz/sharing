//! UpdateExpense repo semantics against a real MySQL: legs replaced and
//! balances re-applied atomically (old effect reversed, new applied).
//! `DATABASE_URL=... cargo test --test update_expense_test -- --ignored`

use sharing::manager::repository::SharingRepository;
use sharing::pb::service::sharing::SplitMethod;
use sqlx::{MySqlPool, Row};

async fn balance(pool: &MySqlPool, budget: &str, user: &str) -> i64 {
    sqlx::query(
        "SELECT CAST(COALESCE(SUM(net_balance),0) AS SIGNED) AS b FROM sharing_balances WHERE budget_id=? AND user_id=?",
    )
    .bind(budget)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
    .get::<i64, _>("b")
}

#[tokio::test]
#[ignore = "needs DATABASE_URL"]
async fn update_reapplies_balances() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let repo = SharingRepository::new(&url).await.unwrap();
    let pool = MySqlPool::connect(&url).await.unwrap();
    let budget = uuid::Uuid::new_v4().to_string();
    let (a, b, c) = ("ua", "ub", "uc");

    // A pays 300, split equally A/B/C (100 each).
    let legs = vec![
        (a.to_string(), 100, 0),
        (b.to_string(), 100, 0),
        (c.to_string(), 100, 0),
    ];
    let e = repo
        .create_expense(
            &budget,
            a,
            300,
            "dinner",
            "2026-10-23",
            None,
            SplitMethod::Equal,
            &legs,
            a,
        )
        .await
        .unwrap();
    assert_eq!(balance(&pool, &budget, a).await, 200);
    assert_eq!(balance(&pool, &budget, b).await, -100);

    // Edit: B pays 400, split between A and B only (200 each).
    let legs2 = vec![(a.to_string(), 200, 0), (b.to_string(), 200, 0)];
    let u = repo
        .update_expense(
            &e.id,
            b,
            400,
            "dinner v2",
            "2026-10-24",
            None,
            SplitMethod::Equal,
            &legs2,
            &[],
        )
        .await
        .unwrap();
    assert_eq!(u.total_amount, 400);
    assert_eq!(u.paid_by, b);
    assert_eq!(repo.get_legs(&e.id).await.unwrap().len(), 2);
    assert_eq!(balance(&pool, &budget, a).await, -200);
    assert_eq!(balance(&pool, &budget, b).await, 200);
    assert_eq!(balance(&pool, &budget, c).await, 0);
    // Net of all balances stays zero.
    let sum = balance(&pool, &budget, a).await
        + balance(&pool, &budget, b).await
        + balance(&pool, &budget, c).await;
    assert_eq!(sum, 0);
}
