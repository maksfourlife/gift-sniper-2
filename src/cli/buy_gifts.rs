use std::sync::Arc;

use anyhow::Result;
use serde::Deserialize;
use sqlx::SqlitePool;
use teloxide::Bot;

use crate::{core::buy_gifts, wrapped_client::WrappedClient};

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    phone_numbers: Vec<String>,
    bot_token: String,
    database_url: String,
}

pub async fn process(gift_id: i64, limit: Option<u64>) -> Result<()> {
    let config: Config = envy::from_env()?;

    let pool = Arc::new(SqlitePool::connect(&config.database_url).await?);
    let bot = Arc::new(Bot::new(config.bot_token));

    let mut clients = vec![];

    for phone_number in config.phone_numbers {
        clients.push(Arc::new(
            WrappedClient::new(
                pool.clone(),
                phone_number,
                config.api_id,
                config.api_hash.clone(),
            )
            .await?,
        ));
    }

    buy_gifts(
        &clients,
        bot.clone(),
        pool.clone(),
        vec![gift_id],
        None,
        limit,
    )
    .await?;

    Ok(())
}
