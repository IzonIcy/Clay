use crate::prefix::{cache_dir, ensure_dir};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const FORMULA_API: &str = "https://formulae.brew.sh/api/formula.json";
const INDEX_CACHE_NAME: &str = "formula.json";
const INDEX_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24);
const USER_AGENT: &str = "clay/0.1 (+https://github.com)";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FormulaRecord {
    pub name: String,
    pub desc: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default, rename = "recommended_dependencies")]
    pub recommended_dependencies: Vec<String>,
    #[serde(default, rename = "optional_dependencies")]
    pub optional_dependencies: Vec<String>,
    #[serde(default, rename = "build_dependencies")]
    pub build_dependencies: Vec<String>,
    #[serde(default, rename = "test_dependencies")]
    pub test_dependencies: Vec<String>,
    #[serde(default)]
    pub versions: Option<Versions>,
    #[serde(default)]
    pub bottle: Option<BottleBlock>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Versions {
    pub stable: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BottleBlock {
    #[serde(default)]
    pub stable: Option<BottleStable>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BottleStable {
    #[serde(default)]
    pub files: HashMap<String, BottleFile>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BottleFile {
    pub url: String,
    pub sha256: String,
}

impl FormulaRecord {
    pub fn version(&self) -> Option<String> {
        self.versions.as_ref().and_then(|v| v.stable.clone())
    }

    pub fn bottle_for_platform(&self, platform: &str) -> Result<BottleFile> {
        let bottle = self.bottle.as_ref().context("no bottle data available")?;
        let stable = bottle
            .stable
            .as_ref()
            .context("no stable bottle available")?;
        stable
            .files
            .get(platform)
            .cloned()
            .with_context(|| format!("no bottle for platform '{platform}'"))
    }

    pub fn dependencies(&self, include_recommended: bool) -> Vec<String> {
        let mut deps = self.dependencies.clone();
        if include_recommended {
            deps.extend(self.recommended_dependencies.iter().cloned());
        }
        deps.sort();
        deps.dedup();
        deps
    }

    pub fn bottle_platforms(&self) -> Vec<String> {
        let mut platforms = Vec::new();
        if let Some(bottle) = self.bottle.as_ref().and_then(|b| b.stable.as_ref()) {
            platforms.extend(bottle.files.keys().cloned());
        }
        platforms.sort();
        platforms
    }
}

#[derive(Debug)]
pub struct FormulaIndex {
    formulas: HashMap<String, FormulaRecord>,
}

impl FormulaIndex {
    pub fn load(prefix: &Path) -> Result<Self> {
        if let Ok(cached) = Self::load_cached(prefix, true) {
            return Ok(cached);
        }

        match Self::fetch_remote_raw() {
            Ok(contents) => {
                let index = Self::parse_index(&contents)?;
                Self::write_cache(prefix, &contents)?;
                Ok(index)
            }
            Err(err) => {
                if let Ok(cached) = Self::load_cached(prefix, false) {
                    return Ok(cached);
                }
                Err(err)
            }
        }
    }

    pub fn update(prefix: &Path) -> Result<Self> {
        let contents = Self::fetch_remote_raw()?;
        let index = Self::parse_index(&contents)?;
        Self::write_cache(prefix, &contents)?;
        Ok(index)
    }

    pub fn get(&self, name: &str) -> Result<FormulaRecord> {
        self.formulas
            .get(name)
            .cloned()
            .with_context(|| format!("formula '{name}' not found"))
    }

    pub fn formulas(&self) -> impl Iterator<Item = &FormulaRecord> {
        self.formulas.values()
    }

    #[cfg(test)]
    pub fn from_records_for_tests(records: Vec<FormulaRecord>) -> Self {
        let formulas = records
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect();
        Self { formulas }
    }

    fn load_cached(prefix: &Path, require_fresh: bool) -> Result<Self> {
        let path = index_cache_path(prefix);
        if require_fresh && cache_is_stale(&path)? {
            anyhow::bail!("formula cache is stale");
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading formula cache {}", path.display()))?;
        Self::parse_index(&contents)
    }

    fn fetch_remote_raw() -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(USER_AGENT)
            .build()
            .context("building HTTP client")?;
        let response = client
            .get(FORMULA_API)
            .send()
            .context("fetching formula index")?
            .error_for_status()
            .context("formula API response error")?;
        response.text().context("reading formula index body")
    }

    fn parse_index(contents: &str) -> Result<Self> {
        let formulas: Vec<FormulaRecord> =
            serde_json::from_str(contents).context("parsing formula index JSON")?;
        let mut map = HashMap::new();
        for formula in formulas {
            map.insert(formula.name.clone(), formula);
        }
        Ok(Self { formulas: map })
    }

    fn write_cache(prefix: &Path, contents: &str) -> Result<PathBuf> {
        let root = cache_dir(prefix);
        ensure_dir(&root)?;
        let path = root.join(INDEX_CACHE_NAME);
        std::fs::write(&path, contents)
            .with_context(|| format!("writing formula cache {}", path.display()))?;
        Ok(path)
    }
}

fn index_cache_path(prefix: &Path) -> PathBuf {
    cache_dir(prefix).join(INDEX_CACHE_NAME)
}

fn cache_is_stale(path: &Path) -> Result<bool> {
    let modified = std::fs::metadata(path)
        .with_context(|| format!("reading formula cache metadata {}", path.display()))?
        .modified()
        .with_context(|| format!("reading formula cache modified time {}", path.display()))?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    Ok(age > INDEX_MAX_AGE)
}
