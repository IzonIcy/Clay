use crate::cache::clean_cache;
use crate::doctor::run_doctor;
use crate::formula::{FormulaIndex, FormulaRecord};
use crate::install::{
    cleanup, install_formula, link_formula, list_installed_versions, list_installed_with_versions,
    prefetch_bottles, unlink_formula, uninstall_formula,
};
use crate::prefix::{default_platform, default_prefix};
use crate::tap::{
    add_tap, list_taps, remove_tap, tap_formula_record, tap_formula_record_with_brew,
};
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "clay")]
#[command(about = "Fast Homebrew-compatible package manager", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Install {
        formula: String,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Uninstall {
        formula: String,
    },
    List {
        #[arg(long)]
        versions: bool,
    },
    Outdated,
    Upgrade {
        formula: Option<String>,
    },
    Fetch {
        formulas: Vec<String>,
    },
    Link {
        formula: String,
        #[arg(long)]
        version: Option<String>,
    },
    Unlink {
        formula: String,
        #[arg(long)]
        version: Option<String>,
    },
    Cleanup,
    Search {
        query: String,
        #[arg(long, default_value_t = 25)]
        limit: usize,
        #[arg(long, default_value_t = false)]
        desc: bool,
    },
    Info {
        formula: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Tap {
        #[command(subcommand)]
        command: TapCommands,
    },
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
    Doctor,
}

#[derive(Subcommand, Debug)]
enum TapCommands {
    Add { repo: String },
    List,
    Remove { repo: String },
}

#[derive(Subcommand, Debug)]
enum CacheCommands {
    Clean,
}

impl Cli {
    pub fn dispatch(self) -> Result<()> {
        match self.command {
            Commands::Install {
                formula,
                platform,
                force,
            } => {
                let platform = platform.or_else(default_platform);
                let prefix = default_prefix()?;
                let formula_record = FormulaIndex::fetch()?.get(&formula).or_else(|_| {
                    let tap = tap_formula_record(&formula)?;
                    let brew = if tap.is_none() {
                        tap_formula_record_with_brew(&formula)?
                    } else {
                        None
                    };
                    tap.or(brew)
                        .ok_or_else(|| anyhow::anyhow!("formula '{formula}' not found in core or taps"))
                })?;
                install_formula(&formula_record, &prefix, platform.as_deref(), force)?;
            }
            Commands::Uninstall { formula } => {
                let prefix = default_prefix()?;
                uninstall_formula(&formula, &prefix)?;
            }
            Commands::List { versions } => {
                let prefix = default_prefix()?;
                if versions {
                    let installed = list_installed_with_versions(&prefix)?;
                    for (name, versions) in installed {
                        if versions.is_empty() {
                            println!("{name}");
                        } else {
                            println!("{} {}", name, versions.join(","));
                        }
                    }
                } else {
                    let installed = crate::install::list_installed(&prefix)?;
                    for name in installed {
                        println!("{name}");
                    }
                }
            }
            Commands::Outdated => {
                let prefix = default_prefix()?;
                let index = FormulaIndex::fetch()?;
                for name in crate::install::list_installed(&prefix)? {
                    let versions = list_installed_versions(&prefix, &name)?;
                    if versions.is_empty() {
                        continue;
                    }
                    let installed = versions.last().cloned().unwrap_or_default();
                    let formula = index.get(&name).or_else(|_| {
                        let tap = tap_formula_record(&name)?;
                        let brew = if tap.is_none() {
                            tap_formula_record_with_brew(&name)?
                        } else {
                            None
                        };
                        tap.or(brew)
                            .ok_or_else(|| anyhow::anyhow!("formula '{name}' not found in core or taps"))
                    })?;
                    if let Some(latest) = formula.version() {
                        if installed != latest {
                            println!("{} {} -> {}", name, installed, latest);
                        }
                    }
                }
            }
            Commands::Upgrade { formula } => {
                let prefix = default_prefix()?;
                let index = FormulaIndex::fetch()?;
                let targets = if let Some(name) = formula {
                    vec![name]
                } else {
                    crate::install::list_installed(&prefix)?
                };
                for name in targets {
                    let formula_record = index.get(&name).or_else(|_| {
                        let tap = tap_formula_record(&name)?;
                        let brew = if tap.is_none() {
                            tap_formula_record_with_brew(&name)?
                        } else {
                            None
                        };
                        tap.or(brew)
                            .ok_or_else(|| anyhow::anyhow!("formula '{name}' not found in core or taps"))
                    })?;
                    install_formula(
                        &formula_record,
                        &prefix,
                        default_platform().as_deref(),
                        true,
                    )?;
                }
            }
            Commands::Fetch { formulas } => {
                if formulas.is_empty() {
                    bail!("no formula names provided");
                }
                let prefix = default_prefix()?;
                let index = FormulaIndex::fetch()?;
                let mut records = Vec::new();
                for name in formulas {
                    let formula_record = index.get(&name).or_else(|_| {
                        let tap = tap_formula_record(&name)?;
                        let brew = if tap.is_none() {
                            tap_formula_record_with_brew(&name)?
                        } else {
                            None
                        };
                        tap.or(brew)
                            .ok_or_else(|| anyhow::anyhow!("formula '{name}' not found in core or taps"))
                    })?;
                    records.push(formula_record);
                }
                prefetch_bottles(&records, &prefix, default_platform().as_deref())?;
            }
            Commands::Link { formula, version } => {
                let prefix = default_prefix()?;
                let linked = link_formula(&prefix, &formula, version.as_deref())?;
                println!("linked {linked} entries");
            }
            Commands::Unlink { formula, version } => {
                let prefix = default_prefix()?;
                let unlinked = unlink_formula(&prefix, &formula, version.as_deref())?;
                println!("unlinked {unlinked} entries");
            }
            Commands::Cleanup => {
                let prefix = default_prefix()?;
                let report = cleanup(&prefix)?;
                println!(
                    "removed {} old versions, {} broken links, {} registry entries",
                    report.removed_versions, report.removed_links, report.removed_registry_entries
                );
            }
            Commands::Search { query, limit, desc } => {
                let index = FormulaIndex::fetch()?;
                let mut matches = Vec::new();
                for formula in index.formulas() {
                    let name_hit = formula.name.contains(&query);
                    let desc_hit = formula
                        .desc
                        .as_ref()
                        .map(|d| d.contains(&query))
                        .unwrap_or(false);
                    if name_hit || (desc && desc_hit) {
                        matches.push(formula);
                        if matches.len() >= limit {
                            break;
                        }
                    }
                }
                for formula in matches {
                    if desc {
                        let desc = formula.desc.as_deref().unwrap_or("");
                        println!("{} - {}", formula.name, desc);
                    } else {
                        println!("{}", formula.name);
                    }
                }
            }
            Commands::Info { formula, json } => {
                let record = FormulaIndex::fetch()?.get(&formula).or_else(|_| {
                    let tap = tap_formula_record(&formula)?;
                    let brew = if tap.is_none() {
                        tap_formula_record_with_brew(&formula)?
                    } else {
                        None
                    };
                    tap.or(brew)
                        .ok_or_else(|| anyhow::anyhow!("formula '{formula}' not found in core or taps"))
                })?;
                if json {
                    let out = serde_json::to_string_pretty(&record)?;
                    println!("{out}");
                } else {
                    print_formula_info(&record);
                }
            }
            Commands::Tap { command } => match command {
                TapCommands::Add { repo } => add_tap(&repo)?,
                TapCommands::List => {
                    for tap in list_taps()? {
                        println!("{tap}");
                    }
                }
                TapCommands::Remove { repo } => remove_tap(&repo)?,
            },
            Commands::Cache { command } => match command {
                CacheCommands::Clean => {
                    let prefix = default_prefix()?;
                    let removed = clean_cache(&prefix)?;
                    println!("removed {removed} cached files");
                }
            },
            Commands::Doctor => {
                run_doctor()?;
            }
        }
        Ok(())
    }
}

fn print_formula_info(record: &FormulaRecord) {
    println!("name: {}", record.name);
    if let Some(desc) = &record.desc {
        println!("desc: {}", desc);
    }
    if let Some(homepage) = &record.homepage {
        println!("homepage: {}", homepage);
    }
    if let Some(version) = record.version() {
        println!("version: {}", version);
    }
    if let Some(license) = &record.license {
        println!("license: {}", license);
    }
}
