use crate::prefix::{cache_dir, default_platform, default_prefix, taps_dir};
use anyhow::Result;

pub fn run_doctor() -> Result<()> {
    let prefix = default_prefix()?;
    let platform = default_platform();
    println!("prefix: {}", prefix.display());
    println!("platform: {}", platform.unwrap_or_else(|| "unknown".to_string()));
    println!("cache: {}", cache_dir(&prefix).display());
    println!("taps: {}", taps_dir(&prefix).display());

    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("git: {}", if git_ok { "ok" } else { "missing" });
    Ok(())
}
