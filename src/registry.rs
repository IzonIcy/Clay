use crate::version::compare_versions;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallRecord {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub installed_at: String,
    #[serde(default)]
    pub links: Vec<PathBuf>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default = "default_requested")]
    pub requested: bool,
}

impl Default for InstallRecord {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            platform: String::new(),
            installed_at: String::new(),
            links: Vec::new(),
            dependencies: Vec::new(),
            requested: true,
        }
    }
}

fn default_requested() -> bool {
    true
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
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating registry directory {}", parent.display()))?;
        }
        let contents = serde_json::to_string_pretty(self).context("serializing registry")?;
        let tmp_path = path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp_path)
                .with_context(|| format!("writing registry {}", tmp_path.display()))?;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("writing registry {}", tmp_path.display()))?;
            file.write_all(b"\n")
                .with_context(|| format!("writing registry {}", tmp_path.display()))?;
            file.sync_all()
                .with_context(|| format!("syncing registry {}", tmp_path.display()))?;
        }
        std::fs::rename(&tmp_path, path).with_context(|| {
            format!(
                "replacing registry {} with {}",
                path.display(),
                tmp_path.display()
            )
        })?;
        Ok(())
    }

    pub fn upsert(&mut self, mut record: InstallRecord) {
        if let Some(existing) = self
            .installs
            .iter()
            .find(|item| item.name == record.name && item.version == record.version)
        {
            record.requested = record.requested || existing.requested;
        }
        record.dependencies.sort();
        record.dependencies.dedup();
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
        entries.sort_by(|a, b| compare_versions(&a.version, &b.version));
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

    pub fn dependents_of(&self, name: &str) -> Vec<String> {
        let mut dependents: Vec<String> = self
            .installs
            .iter()
            .filter(|item| item.name != name && item.dependencies.iter().any(|dep| dep == name))
            .map(|item| item.name.clone())
            .collect();
        dependents.sort();
        dependents.dedup();
        dependents
    }

    pub fn leaves(&self) -> Vec<String> {
        let mut dependencies = std::collections::HashSet::new();
        for entry in &self.installs {
            dependencies.extend(entry.dependencies.iter().cloned());
        }

        let mut leaves: Vec<String> = self
            .installs
            .iter()
            .filter(|entry| !dependencies.contains(&entry.name))
            .map(|entry| entry.name.clone())
            .collect();
        leaves.sort();
        leaves.dedup();
        leaves
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallRecord, Registry};

    fn record(name: &str, version: &str, requested: bool, dependencies: &[&str]) -> InstallRecord {
        InstallRecord {
            name: name.to_string(),
            version: version.to_string(),
            requested,
            dependencies: dependencies.iter().map(|dep| dep.to_string()).collect(),
            ..InstallRecord::default()
        }
    }

    #[test]
    fn upsert_preserves_requested_installs() {
        let mut registry = Registry::default();
        registry.upsert(record("openssl@3", "3.0", true, &[]));
        registry.upsert(record("openssl@3", "3.1", false, &[]));
        registry.upsert(record("openssl@3", "3.1", false, &[]));

        assert!(registry.entries_for("openssl@3")[0].requested);
        assert!(!registry.entries_for("openssl@3")[1].requested);
    }

    #[test]
    fn finds_dependents_and_leaves() {
        let mut registry = Registry::default();
        registry.upsert(record("openssl@3", "3.0", false, &[]));
        registry.upsert(record("wget", "1.0", true, &["openssl@3"]));
        registry.upsert(record("ripgrep", "14.0", true, &[]));

        assert_eq!(registry.dependents_of("openssl@3"), vec!["wget"]);
        assert_eq!(registry.leaves(), vec!["ripgrep", "wget"]);
    }
}
