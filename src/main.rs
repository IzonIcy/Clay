mod bottle;
mod cache;
mod cli;
mod doctor;
mod formula;
mod install;
mod lock;
mod prefix;
mod registry;
mod tap;
mod version;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.dispatch()
}
