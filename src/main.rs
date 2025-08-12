#![allow(clippy::result_large_err)]

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

mod bot;
mod cli;
mod core;
mod db;
mod wrapped_client;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    Cli::parse().process().await?;

    Ok(())
}
