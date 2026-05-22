use crate::prefix::cache_dir;
use anyhow::Result;
use std::path::Path;

pub fn clean_cache(prefix: &Path) -> Result<usize> {
    let cache_root = cache_dir(prefix);
    if !cache_root.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in std::fs::read_dir(&cache_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}
