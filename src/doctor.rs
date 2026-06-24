use crate::prefix::{cache_dir, cellar, default_platform, default_prefix, registry_path, taps_dir};
use crate::registry::Registry;
use anyhow::Result;
use fs2::available_space;
use std::fs::OpenOptions;

pub fn run_doctor() -> Result<()> {
    let prefix = default_prefix()?;
    let platform = default_platform();
    println!("prefix: {}", prefix.display());
    println!(
        "platform: {}",
        platform.unwrap_or_else(|| "unknown".to_string())
    );
    println!("cache: {}", cache_dir(&prefix).display());
    println!("taps: {}", taps_dir(&prefix).display());
    println!("registry: {}", registry_path(&prefix).display());

    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("git: {}", if git_ok { "ok" } else { "missing" });

    let brew_ok = std::process::Command::new("brew")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("brew: {}", if brew_ok { "ok" } else { "missing" });

    let write_test = registry_path(&prefix).with_extension("write_test");
    let writable = (|| -> Result<()> {
        if let Some(parent) = write_test.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&write_test)?;
        let _ = std::fs::remove_file(&write_test);
        Ok(())
    })();
    println!(
        "prefix writable: {}",
        if writable.is_ok() { "ok" } else { "no" }
    );

    if let Ok(available) = available_space(&prefix) {
        println!("disk free: {}", format_bytes(available));
    }

    let collisions = count_link_collisions(&prefix)?;
    println!("link collisions: {}", collisions);
    let broken = count_broken_links(&prefix)?;
    println!("broken links: {}", broken);

    let registry = Registry::load(&registry_path(&prefix))?;
    let mut missing_cellar = 0usize;
    let mut missing_links = 0usize;
    for entry in registry.installs.iter() {
        let cellar_path = cellar(&prefix).join(&entry.name).join(&entry.version);
        if !cellar_path.exists() {
            missing_cellar += 1;
        }
        for link in entry.links.iter() {
            if !link.exists() {
                missing_links += 1;
            }
        }
    }
    println!("registry entries missing cellar: {}", missing_cellar);
    println!("registry links missing: {}", missing_links);
    Ok(())
}

fn count_link_collisions(prefix: &std::path::Path) -> Result<usize> {
    let mut count = 0usize;
    for dir in ["bin", "lib", "include", "share"] {
        let root = prefix.join(dir);
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                let target = std::fs::read_link(&path)?;
                if !target.to_string_lossy().contains("Cellar/") {
                    count += 1;
                }
            } else {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn count_broken_links(prefix: &std::path::Path) -> Result<usize> {
    let mut count = 0usize;
    for dir in ["bin", "lib", "include", "share"] {
        let root = prefix.join(dir);
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let target = std::fs::read_link(&path)?;
            if !target.exists() {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.1} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{} B", bytes)
    }
}
