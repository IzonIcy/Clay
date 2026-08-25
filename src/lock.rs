use crate::prefix::registry_path;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::path::Path;

#[cfg(target_family = "unix")]
pub fn acquire_install_lock(prefix: &Path) -> Result<std::fs::File> {
    use fs2::FileExt;
    let path = registry_path(prefix).with_extension("lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating lock directory {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening lock file {}", path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("locking {}", path.display()))?;
    Ok(file)
}

#[cfg(not(target_family = "unix"))]
pub fn acquire_install_lock(_prefix: &Path) -> Result<()> {
    // Silent no-op was worse than no function: two concurrent installs on
    // Windows could interleave registry writes and linking, corrupting the
    // very state `rollback` depends on. Fail loudly until LockFileEx support
    // lands: refusing to run is recoverable; a corrupt registry is not.
    anyhow::bail!(
        "install locking is not implemented on this platform yet; \
         refusing to run concurrent-unsafe installs. \
         Only one clay install process may run at a time on Windows."
    )
}
