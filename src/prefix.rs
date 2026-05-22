use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

pub fn default_prefix() -> Result<PathBuf> {
    if let Ok(prefix) = env::var("CLAY_PREFIX") {
        return Ok(PathBuf::from(prefix));
    }
    let prefix = if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") {
        "/opt/homebrew"
    } else if cfg!(target_os = "macos") {
        "/usr/local"
    } else {
        "/usr/local"
    };
    Ok(PathBuf::from(prefix))
}

pub fn cellar(prefix: &Path) -> PathBuf {
    prefix.join("Cellar")
}

pub fn taps_dir(prefix: &Path) -> PathBuf {
    prefix.join("Library").join("Taps")
}

pub fn cache_dir(prefix: &Path) -> PathBuf {
    prefix.join("Library").join("Caches").join("clay")
}

pub fn registry_path(prefix: &Path) -> PathBuf {
    prefix
        .join("Library")
        .join("clay")
        .join("registry.json")
}

pub fn default_platform() -> Option<String> {
    if let Ok(platform) = env::var("CLAY_PLATFORM") {
        return Some(platform);
    }
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Some("arm64_sonoma".to_string())
        } else {
            Some("sonoma".to_string())
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            Some("x86_64_linux".to_string())
        } else if cfg!(target_arch = "aarch64") {
            Some("arm64_linux".to_string())
        } else {
            None
        }
    } else {
        None
    }
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating directory {}", path.display()))?;
    }
    Ok(())
}
