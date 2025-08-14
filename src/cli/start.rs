use std::{collections::BTreeSet, sync::Arc, time::Duration};

use anyhow::Result;
use futures::TryFutureExt;
use grammers_client::grammers_tl_types::{
    enums::{StarGift, payments::StarGifts},
    functions::payments::GetStarGifts,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use teloxide::Bot;

use crate::{
    bot::{notify_gifts, run_bot},
    core::{BuyGiftsDestination, buy_gifts},
    wrapped_client::WrappedClient,
};

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    phone_numbers: Vec<String>,
    admin_usernames: Vec<String>,
    initial_gifts_hash: i32,
    bot_token: String,
    database_url: String,
    max_supply: i32,
    // dest_channel_username: String,
}

// 1. authorize all clients
// 2. poll gift updates every 2-3 seconds
// 3. when new gifts are available:
//      1. send them to all connected admin chats in bot
//      2. filter by supply <= max_supply
//      3. for each account:
//          1. for each gift in sorted by supply:
//              1. buy to channel

pub async fn process(ignore_not_limited: bool, do_buy: bool, buy_limit: Option<u64>) -> Result<()> {
    tracing::debug!(ignore_not_limited, do_buy, buy_limit);

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

    let client = clients
        .first()
        .cloned()
        .expect("expected at least one client");

    // let destination = Arc::new(
    //     MaybeResolvedChannel::Username(config.dest_channel_username)
    //         .as_resolved(&client)
    //         .await?,
    // );
    let buy_dest = Arc::new(BuyGiftsDestination::PeerSelf);

    let _bot_handle = tokio::spawn(
        run_bot(
            bot.clone(),
            pool.clone(),
            clients.clone(),
            config.admin_usernames.into(),
            buy_limit,
            buy_dest.clone(),
        )
        .inspect_err(|err| tracing::error!(?err, "run_bot exited with error")),
    );

    let mut gifts_hash = config.initial_gifts_hash;
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    let mut seen_gift_ids = BTreeSet::new();

    loop {
        let star_gifts = client.invoke(&GetStarGifts { hash: gifts_hash }).await?;
        tracing::debug!(?star_gifts);

        if let StarGifts::Gifts(gifts) = star_gifts {
            gifts_hash = gifts.hash;

            // gifts can't be unique here
            let gifts: Vec<_> = gifts
                .gifts
                .into_iter()
                .filter_map(|gift| match gift {
                    StarGift::Gift(gift) => Some(gift),
                    StarGift::Unique(_) => None,
                })
                .filter(|gift| {
                    (ignore_not_limited || gift.limited)
                        && !gift.sold_out
                        && !seen_gift_ids.contains(&gift.id)
                })
                .collect();

            tracing::debug!(?gifts);

            tokio::spawn(
                notify_gifts(bot.clone(), pool.clone(), client.clone(), gifts.clone()).inspect_err(
                    |err| tracing::error!(?err, "send_notifications finished with error"),
                ),
            );

            let mut gifts: Vec<_> = gifts
                .into_iter()
                .filter(|gift| {
                    gift.availability_total.is_some()
                        && gift.availability_total.unwrap() <= config.max_supply
                })
                .collect();

            gifts.sort_by_key(|gift| gift.availability_total);

            tracing::debug!(filtered_and_sorted_gifts = ?gifts);

            for gift in &gifts {
                seen_gift_ids.insert(gift.id);
            }

            let gift_ids: Vec<_> = gifts.iter().map(|gift| gift.id).collect();
            let gift_prices_map = gifts.iter().map(|gift| (gift.id, gift.stars)).collect();

            tracing::debug!(?gift_ids);

            if !gift_ids.is_empty() && do_buy {
                let buy_gifts_result = buy_gifts(
                    &clients,
                    bot.clone(),
                    pool.clone(),
                    gift_ids,
                    Some(&gift_prices_map),
                    buy_limit,
                    &buy_dest,
                )
                .await;

                if let Err(err) = buy_gifts_result {
                    tracing::error!(?err, "failed to buy gifts");
                }
            }
        }

        if let Err(err) = client.sync_session().await {
            tracing::error!(?err, "failed to sync session");
        }

        interval.tick().await;
    }

    #[allow(unreachable_code)]
    {
        _bot_handle.await??;
        Ok(())
    }
}
