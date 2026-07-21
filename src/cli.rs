use crate::cache::clean_cache;
use crate::doctor::run_doctor;
use crate::formula::{FormulaIndex, FormulaRecord};
use crate::install::{
    autoremove, cleanup, install_formula, is_installed, link_formula, list_installed_versions,
    list_installed_with_versions, prefetch_bottles, uninstall_formula, unlink_formula,
    InstallOptions,
};
use crate::prefix::{default_platform, default_prefix, registry_path};
use crate::registry::Registry;
use crate::tap::{
    add_tap, list_taps, remove_tap, tap_formula_record, tap_formula_record_with_brew, update_taps,
};
use crate::version::compare_versions;
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};

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
        #[arg(long, default_value_t = false)]
        only_deps: bool,
        #[arg(long, default_value_t = false)]
        skip_recommended: bool,
        #[arg(long, default_value_t = false)]
        build_from_source: bool,
        #[arg(long, default_value_t = false)]
        overwrite: bool,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    Uninstall {
        formula: String,
        #[arg(long, default_value_t = false)]
        ignore_dependencies: bool,
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
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    Unlink {
        formula: String,
        #[arg(long)]
        version: Option<String>,
    },
    Cleanup,
    Update,
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
    Leaves,
    Autoremove,
    Pin {
        formula: String,
    },
    Unpin {
        formula: String,
    },
}

#[derive(Subcommand, Debug)]
enum TapCommands {
    Add { repo: String },
    List,
    Remove { repo: String },
    Update,
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
                only_deps,
                skip_recommended,
                build_from_source,
                overwrite,
                dry_run,
            } => {
                let platform = platform.or_else(default_platform);
                let prefix = default_prefix()?;
                let index = FormulaIndex::load(&prefix)?;
                let mut resolver = FormulaResolver::new(&index);
                let root = resolver.resolve(&formula)?;
                let include_recommended = !skip_recommended;
                let plan = build_install_plan(&root, &mut resolver, include_recommended)?;
                let root_name = root.name.clone();
                let mut install_plan = Vec::new();

                for record in plan {
                    if only_deps && record.name == root_name {
                        continue;
                    }
                    let name = formula_name(&record);
                    let already_installed = is_installed(&prefix, name)?;
                    let is_root = record.name == root_name;
                    let should_force = force && is_root;
                    if already_installed && !should_force {
                        continue;
                    }
                    install_plan.push((record, is_root, should_force));
                }

                if dry_run {
                    print_install_plan(&install_plan, platform.as_deref(), include_recommended);
                    return Ok(());
                }

                for (record, is_root, should_force) in install_plan {
                    install_formula(
                        &record,
                        &prefix,
                        InstallOptions {
                            platform: platform.as_deref(),
                            force: should_force,
                            build_from_source,
                            overwrite,
                            requested: is_root,
                            dependencies: normalized_dependencies(&record, include_recommended),
                        },
                    )?;
                }
            }
            Commands::Uninstall {
                formula,
                ignore_dependencies,
            } => {
                let prefix = default_prefix()?;
                uninstall_formula(&formula, &prefix, ignore_dependencies)?;
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
                let index = FormulaIndex::load(&prefix)?;
                for name in crate::install::list_installed(&prefix)? {
                    let versions = list_installed_versions(&prefix, &name)?;
                    if versions.is_empty() {
                        continue;
                    }
                    let installed = versions.last().cloned().unwrap_or_default();
                    let formula = resolve_formula_record(&index, &name)?;
                    if let Some(latest) = formula.version() {
                        if compare_versions(&installed, &latest).is_lt() {
                            println!("{name} {installed} -> {latest}");
                        }
                    }
                }
            }
            Commands::Upgrade { formula } => {
                let prefix = default_prefix()?;
                let index = FormulaIndex::load(&prefix)?;
                let registry = Registry::load(&registry_path(&prefix))?;
                let targets = if let Some(name) = formula {
                    vec![name]
                } else {
                    crate::install::list_installed(&prefix)?
                };
                for name in targets {
                    if registry.is_pinned(&name) {
                        println!("{name} is pinned; skipping upgrade");
                        continue;
                    }
                    let formula_record = resolve_formula_record(&index, &name)?;
                    let requested = registry
                        .entries_for(&name)
                        .iter()
                        .any(|entry| entry.requested);
                    let platform = default_platform();
                    install_formula(
                        &formula_record,
                        &prefix,
                        InstallOptions {
                            platform: platform.as_deref(),
                            force: true,
                            build_from_source: false,
                            overwrite: false,
                            requested,
                            dependencies: normalized_dependencies(&formula_record, true),
                        },
                    )?;
                }
            }
            Commands::Fetch { formulas } => {
                if formulas.is_empty() {
                    bail!("no formula names provided");
                }
                let prefix = default_prefix()?;
                let index = FormulaIndex::load(&prefix)?;
                let mut records = Vec::new();
                for name in formulas {
                    let formula_record = resolve_formula_record(&index, &name)?;
                    records.push(formula_record);
                }
                prefetch_bottles(&records, &prefix, default_platform().as_deref())?;
            }
            Commands::Link {
                formula,
                version,
                overwrite,
            } => {
                let prefix = default_prefix()?;
                let linked = link_formula(&prefix, &formula, version.as_deref(), overwrite)?;
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
            Commands::Update => {
                let prefix = default_prefix()?;
                let index = FormulaIndex::update(&prefix)?;
                println!(
                    "updated formula index ({} formulas)",
                    index.formulas().count()
                );
            }
            Commands::Search { query, limit, desc } => {
                let prefix = default_prefix()?;
                let index = FormulaIndex::load(&prefix)?;
                let mut matches = Vec::new();
                for formula in index.formulas() {
                    let name_hit = formula.name.contains(&query);
                    let desc_hit = formula.desc.as_ref().is_some_and(|d| d.contains(&query));
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
                let prefix = default_prefix()?;
                let index = FormulaIndex::load(&prefix)?;
                let record = resolve_formula_record(&index, &formula)?;
                if json {
                    let out = serde_json::to_string_pretty(&record)?;
                    println!("{out}");
                } else {
                    print_formula_info(&record, &prefix)?;
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
                TapCommands::Update => {
                    let updated = update_taps()?;
                    if updated.is_empty() {
                        println!("no taps to update");
                    } else {
                        println!("updated {} taps", updated.len());
                    }
                }
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
            Commands::Leaves => {
                let prefix = default_prefix()?;
                let registry = Registry::load(&registry_path(&prefix))?;
                for leaf in registry.leaves() {
                    println!("{leaf}");
                }
            }
            Commands::Autoremove => {
                let prefix = default_prefix()?;
                let report = autoremove(&prefix)?;
                println!(
                    "removed {} formulae and {} links",
                    report.removed_formulae, report.removed_links
                );
            }
            Commands::Pin { formula } => {
                let prefix = default_prefix()?;
                let mut registry = Registry::load(&registry_path(&prefix))?;
                registry.pin(&formula);
                registry.save(&registry_path(&prefix))?;
                println!("pinned {formula}");
            }
            Commands::Unpin { formula } => {
                let prefix = default_prefix()?;
                let mut registry = Registry::load(&registry_path(&prefix))?;
                registry.unpin(&formula);
                registry.save(&registry_path(&prefix))?;
                println!("unpinned {formula}");
            }
        }
        Ok(())
    }
}

fn print_formula_info(record: &FormulaRecord, prefix: &std::path::Path) -> Result<()> {
    let installed = list_installed_versions(prefix, formula_name(record))?;
    println!("name: {}", record.name);
    if let Some(desc) = &record.desc {
        println!("desc: {desc}");
    }
    if let Some(homepage) = &record.homepage {
        println!("homepage: {homepage}");
    }
    if let Some(version) = record.version() {
        println!("version: {version}");
    }
    if let Some(license) = &record.license {
        println!("license: {license}");
    }
    let deps = record.dependencies(false);
    if !deps.is_empty() {
        println!("deps: {}", deps.join(", "));
    }
    let recommended = record.recommended_dependencies.clone();
    if !recommended.is_empty() {
        println!("recommended: {}", recommended.join(", "));
    }
    let platforms = record.bottle_platforms();
    if !platforms.is_empty() {
        println!("bottles: {}", platforms.join(", "));
    }
    if installed.is_empty() {
        println!("installed: no");
    } else {
        println!("installed: {}", installed.join(", "));
    }
    Ok(())
}

fn print_install_plan(
    plan: &[(FormulaRecord, bool, bool)],
    platform: Option<&str>,
    include_recommended: bool,
) {
    if plan.is_empty() {
        println!("Nothing to install.");
        return;
    }

    println!("Would install:");
    for (record, is_root, force) in plan {
        let version = record.version().unwrap_or_else(|| "unknown".to_string());
        let marker = if *is_root { "requested" } else { "dependency" };
        let action = if *force { "reinstall" } else { "install" };
        println!(
            "  {} {} ({marker}, {action})",
            formula_name(record),
            version
        );

        let deps = normalized_dependencies(record, include_recommended);
        if !deps.is_empty() {
            println!("    deps: {}", deps.join(", "));
        }
    }

    if let Some(platform) = platform {
        println!("Platform: {platform}");
    } else {
        println!("Platform: source build or unknown");
    }
}

fn normalized_dependencies(record: &FormulaRecord, include_recommended: bool) -> Vec<String> {
    let mut deps: Vec<String> = record
        .dependencies(include_recommended)
        .into_iter()
        .map(|dep| dep.split('/').next_back().unwrap_or(&dep).to_string())
        .collect();
    deps.sort();
    deps.dedup();
    deps
}

fn formula_name(record: &FormulaRecord) -> &str {
    record.name.split('/').next_back().unwrap_or(&record.name)
}

fn resolve_formula_record(index: &FormulaIndex, name: &str) -> Result<FormulaRecord> {
    index.get(name).or_else(|_| {
        let tap = tap_formula_record(name)?;
        let brew = if tap.is_none() {
            tap_formula_record_with_brew(name)?
        } else {
            None
        };
        tap.or(brew)
            .ok_or_else(|| anyhow::anyhow!("formula '{name}' not found in core or taps"))
    })
}

struct FormulaResolver<'a> {
    index: &'a FormulaIndex,
    cache: HashMap<String, FormulaRecord>,
}

impl<'a> FormulaResolver<'a> {
    fn new(index: &'a FormulaIndex) -> Self {
        Self {
            index,
            cache: HashMap::new(),
        }
    }

    fn resolve(&mut self, name: &str) -> Result<FormulaRecord> {
        if let Some(record) = self.cache.get(name) {
            return Ok(record.clone());
        }
        let record = resolve_formula_record(self.index, name)?;
        self.cache.insert(name.to_string(), record.clone());
        Ok(record)
    }
}

fn build_install_plan(
    root: &FormulaRecord,
    resolver: &mut FormulaResolver<'_>,
    include_recommended: bool,
) -> Result<Vec<FormulaRecord>> {
    fn visit(
        name: &str,
        resolver: &mut FormulaResolver<'_>,
        include_recommended: bool,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        ordered: &mut Vec<FormulaRecord>,
    ) -> Result<()> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_string()) {
            bail!("dependency cycle detected at {name}");
        }
        let record = resolver.resolve(name)?;
        for dep in record.dependencies(include_recommended) {
            visit(
                &dep,
                resolver,
                include_recommended,
                visiting,
                visited,
                ordered,
            )?;
        }
        visiting.remove(name);
        visited.insert(name.to_string());
        ordered.push(record);
        Ok(())
    }

    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    visit(
        &root.name,
        resolver,
        include_recommended,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::{build_install_plan, normalized_dependencies, FormulaResolver};
    use crate::formula::{FormulaIndex, FormulaRecord, Versions};

    fn formula(name: &str, deps: &[&str], recommended: &[&str]) -> FormulaRecord {
        FormulaRecord {
            name: name.to_string(),
            desc: None,
            homepage: None,
            license: None,
            dependencies: deps.iter().map(std::string::ToString::to_string).collect(),
            recommended_dependencies: recommended
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            optional_dependencies: Vec::new(),
            build_dependencies: Vec::new(),
            test_dependencies: Vec::new(),
            versions: Some(Versions {
                stable: Some("1.0".to_string()),
            }),
            bottle: None,
        }
    }

    fn index(records: Vec<FormulaRecord>) -> FormulaIndex {
        FormulaIndex::from_records_for_tests(records)
    }

    #[test]
    fn install_plan_orders_dependencies_before_root() {
        let root = formula("root", &["dep-b", "dep-a"], &[]);
        let index = index(vec![
            root.clone(),
            formula("dep-a", &[], &[]),
            formula("dep-b", &["dep-c"], &[]),
            formula("dep-c", &[], &[]),
        ]);
        let mut resolver = FormulaResolver::new(&index);
        let plan = build_install_plan(&root, &mut resolver, false).unwrap();
        let names: Vec<String> = plan.into_iter().map(|record| record.name).collect();

        assert_eq!(names, vec!["dep-a", "dep-c", "dep-b", "root"]);
    }

    #[test]
    fn install_plan_can_include_recommended_dependencies() {
        let root = formula("root", &[], &["recommended"]);
        let index = index(vec![root.clone(), formula("recommended", &[], &[])]);
        let mut resolver = FormulaResolver::new(&index);
        let plan = build_install_plan(&root, &mut resolver, true)
            .expect("install plan should be buildable");
        let names: Vec<String> = plan.into_iter().map(|record| record.name).collect();

        assert_eq!(names, vec!["recommended", "root"]);
    }

    #[test]
    fn normalized_dependencies_strip_tap_prefixes() {
        let record = formula("root", &["homebrew/core/openssl@3", "zlib"], &[]);
        assert_eq!(
            normalized_dependencies(&record, false),
            vec!["openssl@3", "zlib"]
        );
    }
}
