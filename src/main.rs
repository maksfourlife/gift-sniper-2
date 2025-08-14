#![allow(clippy::result_large_err)]

use anyhow::Result;
use clap::Parser;
use tracing_appender::non_blocking;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Cli;

mod bot;
mod cli;
mod core;
mod db;
mod wrapped_client;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    // tracing_subscriber::fmt::init();

    let file_appender = tracing_appender::rolling::hourly("logs", "app.log");
    let (file_nb, _guard) = non_blocking(file_appender);

    let filter = EnvFilter::from_default_env();

    let stderr_layer = fmt::layer().with_ansi(true).with_writer(std::io::stderr);

    let file_layer = fmt::layer().with_ansi(false).with_writer(file_nb);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    Cli::parse().process().await?;

    Ok(())
}
