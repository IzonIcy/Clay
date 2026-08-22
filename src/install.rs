use crate::bottle;
use crate::formula::FormulaRecord;
use crate::lock::acquire_install_lock;
use crate::prefix::{cache_dir, cellar, ensure_dir, registry_path};
use crate::registry::{InstallRecord, Registry};
use crate::version::compare_versions;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CleanupReport {
    pub removed_versions: usize,
    pub removed_links: usize,
    pub removed_registry_entries: usize,
}

pub struct AutoRemoveReport {
    pub removed_formulae: usize,
    pub removed_links: usize,
}

pub struct InstallOptions<'a> {
    pub platform: Option<&'a str>,
    pub force: bool,
    pub build_from_source: bool,
    pub overwrite: bool,
    pub requested: bool,
    pub dependencies: Vec<String>,
}

pub fn prefetch_bottles(
    records: &[FormulaRecord],
    prefix: &Path,
    platform: Option<&str>,
) -> Result<()> {
    let platform = platform.ok_or_else(|| anyhow::anyhow!("unable to determine platform"))?;
    let cache_root = cache_dir(prefix);
    ensure_dir(&cache_root)?;

    #[derive(Clone)]
    struct DownloadTask {
        name: String,
        url: String,
        sha256: String,
        tarball: std::path::PathBuf,
    }

    let mut tasks = Vec::new();
    for record in records {
        let version = record
            .version()
            .ok_or_else(|| anyhow::anyhow!("missing stable version"))?;
        let bottle_file = record.bottle_for_platform(platform)?;
        let formula_name = record.name.split('/').next_back().unwrap_or(&record.name);
        let tarball = bottle::cache_path(&cache_root, formula_name, &version);
        tasks.push(DownloadTask {
            name: record.name.clone(),
            url: bottle_file.url,
            sha256: bottle_file.sha256,
            tarball,
        });
    }

    // One shared bar across the parallel downloads; each finished task ticks it.
    let progress = indicatif::ProgressBar::new(tasks.len() as u64)
        .with_style(
            indicatif::ProgressStyle::with_template("fetching [{bar:24}] {pos}/{len} {msg}")
                .expect("valid progress template"),
        )
        .with_message("bottles");

    tasks.par_iter().try_for_each(|task| -> Result<()> {
        if !task.tarball.exists() {
            bottle::download(&task.url, &task.tarball)
                .with_context(|| format!("downloading {}", task.name))?;
        }
        bottle::verify_sha256(&task.tarball, &task.sha256)
            .with_context(|| format!("verifying {}", task.name))?;
        progress.inc(1);
        Ok(())
    })?;

    progress.finish_and_clear();
    Ok(())
}

pub fn install_formula(
    record: &FormulaRecord,
    prefix: &Path,
    options: InstallOptions<'_>,
) -> Result<()> {
    let _lock = acquire_install_lock(prefix)?;
    let InstallOptions {
        platform,
        force,
        build_from_source,
        overwrite,
        requested,
        dependencies,
    } = options;
    let platform = match platform {
        Some(platform) => Some(platform),
        None => {
            if build_from_source {
                None
            } else {
                return Err(anyhow::anyhow!("unable to determine platform"));
            }
        }
    };
    let bottle_file = match platform {
        Some(platform) => record.bottle_for_platform(platform),
        None => Err(anyhow::anyhow!("unable to determine platform")),
    };
    let bottle_file = match bottle_file {
        Ok(bottle) => bottle,
        Err(err) => {
            if build_from_source {
                return install_from_source(
                    record,
                    prefix,
                    force,
                    overwrite,
                    requested,
                    dependencies,
                )
                .with_context(|| format!("building {} from source", record.name));
            }
            return Err(err);
        }
    };
    let formula_name = record.name.split('/').next_back().unwrap_or(&record.name);
    let version = record
        .version()
        .ok_or_else(|| anyhow::anyhow!("missing stable version"))?;

    let cellar_root = cellar(prefix);
    ensure_dir(&cellar_root)?;

    let cache_root = cache_dir(prefix);
    ensure_dir(&cache_root)?;
    let tarball = bottle::cache_path(&cache_root, formula_name, &version);
    if !tarball.exists() {
        println!("==> Fetching {}", record.name);
        bottle::download(&bottle_file.url, &tarball)?;
    }
    println!("==> Verifying {}", record.name);
    if let Err(err) = bottle::verify_sha256(&tarball, &bottle_file.sha256) {
        let _ = std::fs::remove_file(&tarball);
        return Err(err);
    }

    let install_root = cellar_root.join(formula_name);
    let version_dir = install_root.join(&version);
    if version_dir.exists() {
        if force {
            std::fs::remove_dir_all(&version_dir)
                .with_context(|| format!("removing existing {}", version_dir.display()))?;
        } else {
            bail!("{} {} already installed", record.name, version);
        }
    }

    println!("==> Pouring {} {}", record.name, version);
    bottle::extract(&tarball, &cellar_root)?;
    if !version_dir.exists() {
        bail!(
            "unexpected bottle layout; missing {}",
            version_dir.display()
        );
    }
    println!("==> Linking {}", record.name);
    let links = link_tree(prefix, &version_dir, formula_name, overwrite)?;
    let mut registry = Registry::load(&registry_path(prefix))?;
    registry.upsert(InstallRecord {
        name: formula_name.to_string(),
        version,
        platform: platform.unwrap_or("unknown").to_string(),
        installed_at: Utc::now().to_rfc3339(),
        links,
        dependencies,
        requested,
    });
    registry.save(&registry_path(prefix))?;
    Ok(())
}

pub fn uninstall_formula(name: &str, prefix: &Path, ignore_dependencies: bool) -> Result<()> {
    let _lock = acquire_install_lock(prefix)?;
    let cellar_root = cellar(prefix);
    let install_root = cellar_root.join(name);
    if !install_root.exists() {
        bail!("formula '{name}' is not installed");
    }
    let registry_path = registry_path(prefix);
    let mut registry = Registry::load(&registry_path)?;
    if !ignore_dependencies {
        let dependents = registry.dependents_of(name);
        if !dependents.is_empty() {
            bail!(
                "cannot uninstall {name}; still required by {} (use --ignore-dependencies to force)",
                dependents.join(", ")
            );
        }
    }
    std::fs::remove_dir_all(&install_root)
        .with_context(|| format!("removing {}", install_root.display()))?;
    for entry in registry.entries_for(name) {
        unlink_paths(&entry.links)?;
    }
    registry.remove(name);
    registry.save(&registry_path)?;
    unlink_links_for_formula(prefix, name)?;
    Ok(())
}

pub fn rollback_formula(prefix: &Path, name: &str, target_version: Option<&str>) -> Result<String> {
    let _lock = acquire_install_lock(prefix)?;
    let registry_path = registry_path(prefix);
    let mut registry = Registry::load(&registry_path)?;

    let versions = list_versions_from_cellar(prefix, name)?;
    if versions.is_empty() {
        bail!("{name} is not installed");
    }

    // Current = newest known version (registry entry wins over cellar scan).
    let current = {
        let mut candidates: Vec<String> = registry
            .entries_for(name)
            .into_iter()
            .map(|e| e.version)
            .collect();
        candidates.sort_by(|a, b| compare_versions(a, b));
        match candidates.last() {
            Some(latest) => latest.clone(),
            None => versions
                .last()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no installed versions for {name}"))?,
        }
    };

    let older: Vec<String> = versions
        .iter()
        .filter(|v| compare_versions(v, &current) == std::cmp::Ordering::Less)
        .cloned()
        .collect();
    if older.is_empty() {
        bail!("{name} {current} is the oldest version in the cellar; nothing to roll back to");
    }

    let target = match target_version {
        Some(requested) => {
            if !older.iter().any(|v| v == requested) {
                bail!(
                    "{name} {requested} is not an older version in the cellar (available: {})",
                    older.join(", ")
                );
            }
            requested.to_string()
        }
        None => older.last().cloned().expect("checked non-empty above"),
    };

    // Preserve metadata from the current entry before clearing its links.
    let previous_entry = registry
        .entries_for(name)
        .into_iter()
        .find(|entry| entry.version == current);

    for entry in registry.entries_for(name) {
        unlink_paths(&entry.links)?;
    }
    registry
        .installs
        .retain(|entry| !(entry.name == name && entry.version != target));

    let version_dir = cellar(prefix).join(name).join(&target);
    println!("==> Rolling back {name} to {target}");
    let links = link_tree(prefix, &version_dir, name, false)?;

    registry.upsert(InstallRecord {
        name: name.to_string(),
        version: target.clone(),
        platform: previous_entry
            .as_ref()
            .map(|e| e.platform.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        installed_at: Utc::now().to_rfc3339(),
        links,
        dependencies: previous_entry
            .as_ref()
            .map(|e| e.dependencies.clone())
            .unwrap_or_default(),
        requested: previous_entry.as_ref().map(|e| e.requested).unwrap_or(true),
    });
    registry.save(&registry_path)?;

    Ok(target)
}

pub fn list_installed(prefix: &Path) -> Result<Vec<String>> {
    let registry = Registry::load(&registry_path(prefix))?;
    if !registry.installs.is_empty() {
        let mut names: Vec<String> = registry.installs.iter().map(|r| r.name.clone()).collect();
        names.sort();
        names.dedup();
        return Ok(names);
    }
    list_installed_from_cellar(prefix)
}

pub fn list_installed_versions(prefix: &Path, name: &str) -> Result<Vec<String>> {
    let registry = Registry::load(&registry_path(prefix))?;
    let entries = registry.entries_for(name);
    if !entries.is_empty() {
        let mut versions: Vec<String> = entries.into_iter().map(|e| e.version).collect();
        versions.sort_by(|a, b| compare_versions(a, b));
        return Ok(versions);
    }
    list_versions_from_cellar(prefix, name)
}

pub fn list_installed_with_versions(prefix: &Path) -> Result<Vec<(String, Vec<String>)>> {
    let registry = Registry::load(&registry_path(prefix))?;
    if !registry.installs.is_empty() {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in registry.installs {
            map.entry(entry.name).or_default().push(entry.version);
        }
        let mut out: Vec<(String, Vec<String>)> = map
            .into_iter()
            .map(|(name, mut versions)| {
                versions.sort_by(|a, b| compare_versions(a, b));
                (name, versions)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(out);
    }

    let mut out = Vec::new();
    for name in list_installed_from_cellar(prefix)? {
        let versions = list_versions_from_cellar(prefix, &name)?;
        out.push((name, versions));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub fn link_formula(
    prefix: &Path,
    name: &str,
    version: Option<&str>,
    overwrite: bool,
) -> Result<usize> {
    let _lock = acquire_install_lock(prefix)?;
    let version = resolve_version(prefix, name, version)?;
    let version_dir = cellar(prefix).join(name).join(&version);
    if !version_dir.exists() {
        bail!("{name} {version} is not installed");
    }
    let new_links = link_tree(prefix, &version_dir, name, overwrite)?;
    let mut registry = Registry::load(&registry_path(prefix))?;
    let mut matched = false;
    for entry in &mut registry.installs {
        if entry.name == name && entry.version == version {
            merge_links(&mut entry.links, &new_links);
            matched = true;
            break;
        }
    }
    if !matched {
        registry.installs.push(InstallRecord {
            name: name.to_string(),
            version,
            platform: "unknown".to_string(),
            installed_at: Utc::now().to_rfc3339(),
            links: new_links.clone(),
            dependencies: Vec::new(),
            requested: true,
        });
    }
    registry.save(&registry_path(prefix))?;
    Ok(new_links.len())
}

pub fn unlink_formula(prefix: &Path, name: &str, version: Option<&str>) -> Result<usize> {
    let _lock = acquire_install_lock(prefix)?;
    let registry_path = registry_path(prefix);
    let mut registry = Registry::load(&registry_path)?;
    let mut removed = 0usize;
    if !registry.installs.is_empty() {
        for entry in &mut registry.installs {
            let matches_name = entry.name == name;
            let matches_version = version.is_none_or(|v| v == entry.version);
            if matches_name && matches_version {
                removed += unlink_paths(&entry.links)?;
                entry.links.clear();
            }
        }
        registry.save(&registry_path)?;
        return Ok(removed);
    }
    removed += unlink_links_for_formula(prefix, name)?;
    Ok(removed)
}

pub fn is_installed(prefix: &Path, name: &str) -> Result<bool> {
    let registry = Registry::load(&registry_path(prefix))?;
    if registry.installs.iter().any(|entry| entry.name == name) {
        return Ok(true);
    }
    Ok(cellar(prefix).join(name).exists())
}

pub fn cleanup(prefix: &Path) -> Result<CleanupReport> {
    let _lock = acquire_install_lock(prefix)?;
    let mut removed_versions = 0usize;
    let mut removed_links = 0usize;
    let mut removed_registry_entries = 0usize;
    let cellar_root = cellar(prefix);
    let registry_path = registry_path(prefix);
    let mut registry = Registry::load(&registry_path)?;

    for formula in list_installed_from_cellar(prefix)? {
        let mut versions = list_versions_from_cellar(prefix, &formula)?;
        if versions.len() <= 1 {
            continue;
        }
        versions.sort_by(|a, b| compare_versions(a, b));
        let keep = versions.pop().unwrap_or_default();
        for version in versions {
            let version_path = cellar_root.join(&formula).join(&version);
            if version_path.exists() {
                std::fs::remove_dir_all(&version_path)
                    .with_context(|| format!("removing {}", version_path.display()))?;
                removed_versions += 1;
            }
            let before = registry.installs.len();
            registry
                .installs
                .retain(|item| !(item.name == formula && item.version == version));
            removed_registry_entries += before - registry.installs.len();
        }

        let before = registry.installs.len();
        registry
            .installs
            .retain(|item| !(item.name == formula && item.version != keep));
        removed_registry_entries += before - registry.installs.len();
    }

    let before = registry.installs.len();
    registry
        .installs
        .retain(|item| cellar_root.join(&item.name).join(&item.version).exists());
    removed_registry_entries += before - registry.installs.len();
    registry.save(&registry_path)?;

    removed_links += remove_broken_links(prefix)?;

    Ok(CleanupReport {
        removed_versions,
        removed_links,
        removed_registry_entries,
    })
}

pub fn autoremove(prefix: &Path) -> Result<AutoRemoveReport> {
    let _lock = acquire_install_lock(prefix)?;
    let registry_path = registry_path(prefix);
    let mut registry = Registry::load(&registry_path)?;
    let cellar_root = cellar(prefix);
    let mut removed_formulae = 0usize;
    let mut removed_links = 0usize;

    loop {
        let leaves: HashSet<String> = registry.leaves().into_iter().collect();
        let targets: Vec<String> = registry
            .installs
            .iter()
            .filter(|entry| !entry.requested && leaves.contains(&entry.name))
            .map(|entry| entry.name.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        if targets.is_empty() {
            break;
        }

        for name in targets {
            for entry in registry.entries_for(&name) {
                removed_links += unlink_paths(&entry.links)?;
            }
            let install_root = cellar_root.join(&name);
            if install_root.exists() {
                std::fs::remove_dir_all(&install_root)
                    .with_context(|| format!("removing {}", install_root.display()))?;
            }
            registry.remove(&name);
            removed_formulae += 1;
        }
    }

    registry.save(&registry_path)?;
    Ok(AutoRemoveReport {
        removed_formulae,
        removed_links,
    })
}

fn link_tree(
    prefix: &Path,
    version_dir: &Path,
    formula: &str,
    overwrite: bool,
) -> Result<Vec<std::path::PathBuf>> {
    let mut links = Vec::new();
    let mut conflicts = Vec::new();
    let needle = format!("Cellar/{formula}");
    for dir in ["bin", "lib", "include", "share"] {
        let from_dir = version_dir.join(dir);
        if !from_dir.exists() {
            continue;
        }
        let to_dir = prefix.join(dir);
        ensure_dir(&to_dir)?;
        for entry in std::fs::read_dir(from_dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name();
            let link_path = to_dir.join(file_name);
            if link_path.symlink_metadata().is_ok() {
                let metadata = std::fs::symlink_metadata(&link_path)?;
                if metadata.file_type().is_symlink() {
                    if let Ok(target) = std::fs::read_link(&link_path) {
                        if target.to_string_lossy().contains(&needle) {
                            std::fs::remove_file(&link_path).with_context(|| {
                                format!("removing link {}", link_path.display())
                            })?;
                        } else if overwrite {
                            std::fs::remove_file(&link_path).with_context(|| {
                                format!("removing link {}", link_path.display())
                            })?;
                        } else {
                            conflicts.push(link_path);
                            continue;
                        }
                    } else {
                        conflicts.push(link_path);
                        continue;
                    }
                } else {
                    conflicts.push(link_path);
                    continue;
                }
            }
            #[cfg(target_family = "unix")]
            std::os::unix::fs::symlink(&path, &link_path)
                .with_context(|| format!("linking {}", link_path.display()))?;
            links.push(link_path);
        }
    }
    if !conflicts.is_empty() {
        eprintln!(
            "warning: {} existing paths blocked linking (use --overwrite to replace symlinks)",
            conflicts.len()
        );
        for path in conflicts.iter().take(8) {
            eprintln!("  {}", path.display());
        }
        if conflicts.len() > 8 {
            eprintln!("  ... and {} more", conflicts.len() - 8);
        }
    }
    Ok(links)
}

fn unlink_paths(paths: &[std::path::PathBuf]) -> Result<usize> {
    let mut removed = 0usize;
    for path in paths {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("removing link {}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn list_installed_from_cellar(prefix: &Path) -> Result<Vec<String>> {
    let cellar_root = cellar(prefix);
    if !cellar_root.exists() {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for entry in std::fs::read_dir(cellar_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            installed.push(name);
        }
    }
    installed.sort();
    Ok(installed)
}

fn list_versions_from_cellar(prefix: &Path, name: &str) -> Result<Vec<String>> {
    let cellar_root = cellar(prefix);
    let install_root = cellar_root.join(name);
    if !install_root.exists() {
        return Ok(Vec::new());
    }
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(install_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            versions.push(name);
        }
    }
    versions.sort_by(|a, b| compare_versions(a, b));
    Ok(versions)
}

fn unlink_links_for_formula(prefix: &Path, formula: &str) -> Result<usize> {
    let mut removed = 0usize;
    let needle = format!("Cellar/{formula}");
    for dir in ["bin", "lib", "include", "share"] {
        let root = prefix.join(dir);
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if let Ok(target) = std::fs::read_link(&path) {
                if target.to_string_lossy().contains(&needle) {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("removing link {}", path.display()))?;
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}

fn remove_broken_links(prefix: &Path) -> Result<usize> {
    let mut removed = 0usize;
    for dir in ["bin", "lib", "include", "share"] {
        let root = prefix.join(dir);
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let target = std::fs::read_link(&path)?;
            if !target.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing link {}", path.display()))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn merge_links(existing: &mut Vec<PathBuf>, new_links: &[PathBuf]) {
    let mut seen: HashSet<PathBuf> = existing.iter().cloned().collect();
    for link in new_links {
        if seen.insert(link.clone()) {
            existing.push(link.clone());
        }
    }
}

fn resolve_version(prefix: &Path, name: &str, version: Option<&str>) -> Result<String> {
    if let Some(version) = version {
        return Ok(version.to_string());
    }

    let registry = Registry::load(&registry_path(prefix))?;
    let mut versions: Vec<String> = registry
        .entries_for(name)
        .into_iter()
        .map(|entry| entry.version)
        .collect();
    if versions.is_empty() {
        versions = list_versions_from_cellar(prefix, name)?;
    }
    versions.sort_by(|a, b| compare_versions(a, b));
    versions
        .last()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no installed versions for {name}"))
}

fn install_from_source(
    record: &FormulaRecord,
    prefix: &Path,
    force: bool,
    overwrite: bool,
    requested: bool,
    dependencies: Vec<String>,
) -> Result<()> {
    let formula_name = record.name.split('/').next_back().unwrap_or(&record.name);
    let mut cmd = Command::new("brew");
    if force {
        cmd.arg("reinstall");
    } else {
        cmd.arg("install");
    }
    let status = cmd
        .arg("--build-from-source")
        .arg(&record.name)
        .status()
        .context("brew install failed")?;
    if !status.success() {
        bail!("brew install failed for {}", record.name);
    }

    let mut version = record.version().unwrap_or_default();
    if version.is_empty() || !cellar(prefix).join(formula_name).join(&version).exists() {
        let mut versions = list_versions_from_cellar(prefix, formula_name)?;
        versions.sort_by(|a, b| compare_versions(a, b));
        version = versions
            .last()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no installed versions for {formula_name}"))?;
    }
    let version_dir = cellar(prefix).join(formula_name).join(&version);
    if !version_dir.exists() {
        bail!("{formula_name} {version} is not installed after source build");
    }
    println!("==> Linking {}", record.name);
    let links = link_tree(prefix, &version_dir, formula_name, overwrite)?;
    let mut registry = Registry::load(&registry_path(prefix))?;
    registry.upsert(InstallRecord {
        name: formula_name.to_string(),
        version,
        platform: "source".to_string(),
        installed_at: Utc::now().to_rfc3339(),
        links,
        dependencies,
        requested,
    });
    registry.save(&registry_path(prefix))?;
    Ok(())
}
