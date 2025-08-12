use anyhow::Result;
use clap::{Parser, Subcommand};

mod buy_gifts;
mod start;

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Start(Start),
    BuyGift(BuyGift),
}

#[derive(Debug, Parser)]
struct Start {
    #[clap(long)]
    ignore_not_limited: bool,
    #[clap(long)]
    do_buy: bool,
    #[clap(long)]
    buy_limit: Option<u64>,
}

#[derive(Debug, Parser)]
struct BuyGift {
    gift_id: i64,
    limit: Option<u64>,
}

impl Cli {
    pub async fn process(self) -> Result<()> {
        match self.command {
            Command::Start(Start {
                ignore_not_limited,
                do_buy,
                buy_limit,
            }) => start::process(ignore_not_limited, do_buy, buy_limit).await,
            Command::BuyGift(BuyGift { gift_id, limit }) => {
                buy_gifts::process(gift_id, limit).await
            }
        }
    }
}
