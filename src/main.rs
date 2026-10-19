mod config;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// TOML configuration file (also accepts TEMPORAT_CONFIG).
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let config = config::load(Args::parse().config)?;
    println!(
        "Configuration loaded for namespace {}",
        config.connection.namespace
    );
    Ok(())
}
