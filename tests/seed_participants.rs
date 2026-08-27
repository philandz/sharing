//! Seed `sharing_participants` rows for a given budget_id + user_id.
//! Used by smoke to give the participants list real data to verify.
//!
//! Run with:
//!   DATABASE_URL=... BUDGET_ID=... USER_ID=... cargo test --test seed_participants -- --ignored --nocapture
use sqlx::{MySqlPool, Row};
use std::env;

#[tokio::test]
#[ignore = "manual maintenance — uses BUDGET_ID + USER_ID env vars"]
async fn seed() {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL");
    let budget_id = env::var("BUDGET_ID").expect("BUDGET_ID");
    let user_id = env::var("USER_ID").expect("USER_ID");
    let pool = MySqlPool::connect(&url).await.expect("connect");
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO sharing_participants (id, budget_id, user_id, role, display_name, joined_at, last_seen_at)
         VALUES (?, ?, ?, 'member', 'Smoke User', ?, ?)"
    )
    .bind(&id).bind(&budget_id).bind(&user_id)
    .bind(now).bind(now)
    .execute(&pool).await.expect("insert");
    eprintln!(
        "seeded sharing_participant id={} budget={} user={}",
        id, budget_id, user_id
    );
    // Verify
    let rows = sqlx::query("SELECT COUNT(*) as c FROM sharing_participants WHERE budget_id = ?")
        .bind(&budget_id)
        .fetch_one(&pool)
        .await
        .expect("count");
    let c: i64 = rows.get("c");
    eprintln!("sharing_participants count for budget={}: {}", budget_id, c);
}
