mod app;
mod config;
mod debug_log;
mod details;
mod details_view;
mod error_popup;
mod history;
mod input;
mod layout;
mod payload;
mod query;
mod query_view;
mod runtime;
mod saved_queries;
mod temporal;
mod temporal_details;
mod text_input;
mod theme;
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
    /// Startup profile (overrides default_profile in TOML).
    #[arg(long)]
    profile: Option<String>,
    /// Check the Temporal connection without opening the TUI.
    #[arg(long)]
    check_connection: bool,
    /// Preview configuration and keybindings without connecting to Temporal.
    #[arg(long, conflicts_with = "check_connection")]
    offline: bool,
    /// Write connection diagnostics and full error chains to a private log file.
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut config = config::load(args.config.clone())?;
    if let Some(profile) = &args.profile {
        config.select_profile(profile.clone())?;
    }
    let log_path = if args.debug {
        let path = debug_log::init(
            config
                .profiles
                .values()
                .filter_map(|profile| profile.api_key.as_deref()),
        )?;
        eprintln!("Debug log: {}", path.display());
        debug_log::record(
            "startup",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH, "offline": args.offline,
                "check_connection": args.check_connection,
            }),
        );
        Some(path)
    } else {
        None
    };
    let result = run(args, config).await;
    if let Err(error) = &result {
        debug_log::failure("application", error);
    }
    debug_log::record("shutdown", serde_json::json!({ "success": result.is_ok() }));
    if let Some(path) = log_path {
        eprintln!("Debug log: {}", path.display());
    }
    result
}

async fn run(args: Args, config: config::Config) -> Result<()> {
    if args.check_connection {
        let client = temporal::connect(config.connection()).await?;
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
