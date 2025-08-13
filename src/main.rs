#![allow(clippy::result_large_err)]

use std::{fs::File, io::BufWriter};

use anyhow::Result;
use clap::Parser;
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

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().with_writer(std::io::stderr).with_ansi(true))
        .with(
            fmt::layer()
                .with_writer(File::create("logs/app.log")?)
                .with_ansi(false),
        )
        .init();

    Cli::parse().process().await?;

    Ok(())
}
