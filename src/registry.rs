use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct InstallRecord {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub installed_at: String,
    pub links: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Registry {
    pub installs: Vec<InstallRecord>,
    #[serde(default)]
    pub pinned: Vec<String>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading registry {}", path.display()))?;
        let registry = serde_json::from_str(&contents)
            .with_context(|| format!("parsing registry {}", path.display()))?;
        Ok(registry)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("creating registry directory {}", parent.display())
            })?;
        }
        let contents = serde_json::to_string_pretty(self)
            .context("serializing registry")?;
        std::fs::write(path, contents)
            .with_context(|| format!("writing registry {}", path.display()))?;
        Ok(())
    }

    pub fn upsert(&mut self, record: InstallRecord) {
        self.installs
            .retain(|item| !(item.name == record.name && item.version == record.version));
        self.installs.push(record);
    }

    pub fn remove(&mut self, name: &str) {
        self.installs.retain(|item| item.name != name);
    }

    pub fn entries_for(&self, name: &str) -> Vec<InstallRecord> {
        let mut entries: Vec<InstallRecord> = self
            .installs
            .iter()
            .filter(|item| item.name == name)
            .cloned()
            .collect();
        entries.sort_by(|a, b| a.version.cmp(&b.version));
        entries
    }

    pub fn pin(&mut self, name: &str) {
        if !self.pinned.iter().any(|p| p == name) {
            self.pinned.push(name.to_string());
            self.pinned.sort();
            self.pinned.dedup();
        }
    }

    pub fn unpin(&mut self, name: &str) {
        self.pinned.retain(|item| item != name);
    }

    pub fn is_pinned(&self, name: &str) -> bool {
        self.pinned.iter().any(|item| item == name)
    }
}
