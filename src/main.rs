use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = clay::cli::Cli::parse();
    cli.dispatch()
}
