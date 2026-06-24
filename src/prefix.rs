use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn default_prefix() -> Result<PathBuf> {
    if let Ok(prefix) = env::var("CLAY_PREFIX") {
        return Ok(PathBuf::from(prefix));
    }
    let prefix = if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") {
        "/opt/homebrew"
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
    prefix.join("Library").join("clay").join("registry.json")
}

pub fn default_platform() -> Option<String> {
    if let Ok(platform) = env::var("CLAY_PLATFORM") {
        return Some(platform);
    }
    if cfg!(target_os = "macos") {
        let tag = macos_bottle_tag().unwrap_or("sonoma");
        if cfg!(target_arch = "aarch64") {
            Some(format!("arm64_{tag}"))
        } else {
            Some(tag.to_string())
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

fn macos_bottle_tag() -> Option<&'static str> {
    let output = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let major = version
        .trim()
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())?;
    macos_bottle_tag_for_major(major)
}

fn macos_bottle_tag_for_major(major: u32) -> Option<&'static str> {
    match major {
        26.. => Some("tahoe"),
        15 => Some("sequoia"),
        14 => Some("sonoma"),
        13 => Some("ventura"),
        12 => Some("monterey"),
        11 => Some("big_sur"),
        _ => None,
    }
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating directory {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::macos_bottle_tag_for_major;

    #[test]
    fn maps_macos_major_versions_to_homebrew_tags() {
        assert_eq!(macos_bottle_tag_for_major(26), Some("tahoe"));
        assert_eq!(macos_bottle_tag_for_major(15), Some("sequoia"));
        assert_eq!(macos_bottle_tag_for_major(14), Some("sonoma"));
        assert_eq!(macos_bottle_tag_for_major(11), Some("big_sur"));
        assert_eq!(macos_bottle_tag_for_major(10), None);
    }
}
