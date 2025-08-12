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
    Start,
    BuyGift(BuyGift),
}

#[derive(Debug, Parser)]
struct BuyGift {
    gift_id: i64,
}

impl Cli {
    pub async fn process(self) -> Result<()> {
        match self.command {
            Command::Start => start::process().await,
            Command::BuyGift(BuyGift { gift_id }) => buy_gifts::process(gift_id).await,
        }
    }
}
