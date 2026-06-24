use crate::formula::FormulaRecord;
use crate::prefix::{ensure_dir, taps_dir};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn add_tap(repo: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        bail!("tap repo must be in form user/repo");
    }
    let user = parts[0];
    let name = parts[1];
    let root = default_taps_dir()?;
    ensure_dir(&root)?;
    let tap_path = root.join(user).join(format!("homebrew-{name}"));
    if tap_path.exists() {
        bail!("tap {repo} already exists");
    }
    let url = format!("https://github.com/{user}/homebrew-{name}.git");
    let status = Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(&tap_path)
        .status()
        .context("git clone failed")?;
    if !status.success() {
        bail!("git clone failed for {repo}");
    }
    Ok(())
}

pub fn list_taps() -> Result<Vec<String>> {
    let root = default_taps_dir()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut taps = Vec::new();
    for user_entry in std::fs::read_dir(root)? {
        let user_entry = user_entry?;
        if !user_entry.file_type()?.is_dir() {
            continue;
        }
        let user = user_entry.file_name().to_string_lossy().to_string();
        for tap_entry in std::fs::read_dir(user_entry.path())? {
            let tap_entry = tap_entry?;
            if !tap_entry.file_type()?.is_dir() {
                continue;
            }
            let name = tap_entry.file_name().to_string_lossy().to_string();
            let tap_name = name.strip_prefix("homebrew-").unwrap_or(&name);
            taps.push(format!("{user}/{tap_name}"));
        }
    }
    taps.sort();
    Ok(taps)
}

pub fn remove_tap(repo: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        bail!("tap repo must be in form user/repo");
    }
    let user = parts[0];
    let name = parts[1];
    let root = default_taps_dir()?;
    let tap_path = root.join(user).join(format!("homebrew-{name}"));
    if !tap_path.exists() {
        bail!("tap {repo} not found");
    }
    std::fs::remove_dir_all(&tap_path)
        .with_context(|| format!("removing {}", tap_path.display()))?;
    Ok(())
}

pub fn update_taps() -> Result<Vec<String>> {
    let root = default_taps_dir()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut updated = Vec::new();
    for user_entry in std::fs::read_dir(&root)? {
        let user_entry = user_entry?;
        if !user_entry.file_type()?.is_dir() {
            continue;
        }
        let user = user_entry.file_name().to_string_lossy().to_string();
        for tap_entry in std::fs::read_dir(user_entry.path())? {
            let tap_entry = tap_entry?;
            if !tap_entry.file_type()?.is_dir() {
                continue;
            }
            let name = tap_entry.file_name().to_string_lossy().to_string();
            let status = Command::new("git")
                .arg("-C")
                .arg(tap_entry.path())
                .arg("pull")
                .arg("--ff-only")
                .status()
                .context("git pull failed")?;
            if !status.success() {
                bail!("git pull failed for {user}/{name}");
            }
            let tap_name = name.strip_prefix("homebrew-").unwrap_or(&name);
            updated.push(format!("{user}/{tap_name}"));
        }
    }
    updated.sort();
    Ok(updated)
}

fn default_taps_dir() -> Result<PathBuf> {
    let prefix = crate::prefix::default_prefix()?;
    Ok(taps_dir(&prefix))
}

pub fn tap_formula_record(name: &str) -> Result<Option<FormulaRecord>> {
    let root = default_taps_dir()?;
    if !root.exists() {
        return Ok(None);
    }
    let name_stem = name.split('/').next_back().unwrap_or(name);
    for user_entry in std::fs::read_dir(root)? {
        let user_entry = user_entry?;
        if !user_entry.file_type()?.is_dir() {
            continue;
        }
        for tap_entry in std::fs::read_dir(user_entry.path())? {
            let tap_entry = tap_entry?;
            if !tap_entry.file_type()?.is_dir() {
                continue;
            }
            let formula_dir = tap_entry.path().join("Formula");
            if !formula_dir.exists() {
                continue;
            }
            let json_path = formula_dir.join(format!("{name_stem}.json"));
            if json_path.exists() {
                let contents = std::fs::read_to_string(&json_path)
                    .with_context(|| format!("reading tap formula {}", json_path.display()))?;
                let record = serde_json::from_str(&contents)
                    .with_context(|| format!("parsing tap formula {}", json_path.display()))?;
                return Ok(Some(record));
            }
        }
    }
    Ok(None)
}

pub fn tap_formula_record_with_brew(name: &str) -> Result<Option<FormulaRecord>> {
    let output = std::process::Command::new("brew")
        .arg("info")
        .arg("--json=v2")
        .arg(name)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout)?;
    let formulas = value
        .get("formulae")
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow::anyhow!("invalid brew info JSON"))?;
    if formulas.is_empty() {
        return Ok(None);
    }
    let record: FormulaRecord = serde_json::from_value(formulas[0].clone())?;
    Ok(Some(record))
}
