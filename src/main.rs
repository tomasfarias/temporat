mod app;
mod config;
mod input;
mod temporal;
mod view;
mod workflow;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// TOML configuration file (also accepts TEMPORAT_CONFIG).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Check the Temporal connection without opening the TUI.
    #[arg(long)]
    check_connection: bool,
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
            config.table.page_size,
            Vec::new(),
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
    let result = run(&mut terminal, &config);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, config: &config::Config) -> Result<()> {
    let mut model = app::Model::preview(config);
    app::update(&mut model, app::Message::Resize(terminal.size()?.height));
    while !model.quit {
        terminal.draw(|frame| view::view(frame, &model, config))?;
        let message = match event::read()? {
            Event::Key(key) => config.keybindings.action(key).map(app::Message::Action),
            Event::Resize(_, height) => Some(app::Message::Resize(height)),
            _ => None,
        };
        if let Some(message) = message {
            // The preview has no external effects yet.
            let _effect = app::update(&mut model, message);
        }
    }
    Ok(())
}
