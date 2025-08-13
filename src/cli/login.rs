use std::sync::Arc;

use anyhow::Result;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::wrapped_client::WrappedClient;

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    phone_numbers: Vec<String>,
    database_url: String,
}

pub async fn process() -> Result<()> {
    let config: Config = envy::from_env()?;

    let pool = Arc::new(SqlitePool::connect(&config.database_url).await?);

    for phone_number in config.phone_numbers {
        WrappedClient::new(
            pool.clone(),
            phone_number,
            config.api_id,
            config.api_hash.clone(),
        )
        .await?;
    }

    Ok(())
}
