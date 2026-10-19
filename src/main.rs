mod app;
mod config;
mod input;
mod query;
mod query_view;
mod runtime;
mod temporal;
mod text_input;
mod view;
mod workflow;

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// TOML configuration file (also accepts TEMPORAT_CONFIG).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Check the Temporal connection without opening the TUI.
    #[arg(long)]
    check_connection: bool,
    /// Preview configuration and keybindings without connecting to Temporal.
    #[arg(long, conflicts_with = "check_connection")]
    offline: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = config::load(args.config)?;
    if args.check_connection {
        let client = temporal::connect(&config.connection).await?;
        let page = temporal::list_page(
            &client,
            &config.table.columns,
            config.table.page_size.unwrap_or(1),
            Vec::new(),
            config.initial_query(),
        )
        .await?;
        println!(
            "Connected to Temporal; {} workflows in first page (more: {})",
            page.rows.len(),
            !page.next_page_token.is_empty()
        );
        return Ok(());
    }
    let mut terminal = ratatui::init();
    let result = runtime::run(&mut terminal, Arc::new(config), args.offline).await;
    ratatui::restore();
    result
}
