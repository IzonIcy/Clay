use anyhow::Result;
use clap::Parser;
use clay::cli;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.dispatch()
}
