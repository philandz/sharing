//! Apply sharing migrations to a target MySQL schema (typically Aiven
//! philandz). Use to migrate sharing from local MySQL to Aiven.
//!
//! Run with: DATABASE_URL=... cargo test --test apply_migrations -- --ignored --nocapture
use sqlx::{MySqlPool, Row};

const SHARING_MIGRATIONS: &[(&str, &str)] = &[
    ("20260422000001", "create_sharing"),
    ("20260616000002", "settlement_payments"),
    ("20260620000001", "sharing_participants"),
    ("20260622000002", "comments_and_activity"),
    ("20260624000001", "sharing_expense_items"),
];

#[tokio::test]
#[ignore = "manual maintenance — applies DDL to shared DB"]
async fn apply_migrations() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = MySqlPool::connect(&url).await.expect("connect");

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut applied = 0usize;
    for (version_prefix, _label) in SHARING_MIGRATIONS {
        let filename = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.starts_with(version_prefix));
        let Some(filename) = filename else {
            eprintln!("no migration file matching {}*", version_prefix);
            continue;
        };
        let path = dir.join(&filename);
        let body = std::fs::read_to_string(&path).expect("read");
        eprintln!("--- applying {} ---", filename);
        // sqlx::query doesn't accept multi-statement strings; split on `;`.
        for stmt in body.split(';') {
            let t = stmt.trim();
            if t.is_empty() || t.starts_with("--") {
                continue;
            }
            if let Err(e) = sqlx::query(t).execute(&pool).await {
                eprintln!("failed stmt ({}): {}", t.lines().next().unwrap_or(""), e);
            }
        }
        applied += 1;
    }
    eprintln!("applied {} migration files", applied);

    // Verify resulting schema
    let rows = sqlx::query(
        "SELECT TABLE_NAME FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE 'sharing%'
         ORDER BY TABLE_NAME",
    )
    .fetch_all(&pool)
    .await
    .expect("verify");
    for r in rows {
        let n: String = r.get("TABLE_NAME");
        eprintln!("  {}", n);
    }
}
