use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

pub async fn init_db(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create hub data dir {}", data_dir.display()))?;
    let db_path = data_dir.join("hub.db");
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .context("failed to open hub database")?;
    sqlx::migrate!()
        .run(&pool)
        .await
        .context("failed to run hub migrations")?;
    Ok(pool)
}
