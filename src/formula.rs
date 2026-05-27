use crate::prefix::{cache_dir, ensure_dir};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FORMULA_API: &str = "https://formulae.brew.sh/api/formula.json";
const INDEX_CACHE_NAME: &str = "formula.json";

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
        let bottle = self
            .bottle
            .as_ref()
            .context("no bottle data available")?;
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
        if let Ok(cached) = Self::load_cached(prefix) {
            return Ok(cached);
        }
        let contents = Self::fetch_remote_raw()?;
        let index = Self::parse_index(&contents)?;
        Self::write_cache(prefix, &contents)?;
        Ok(index)
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

    fn load_cached(prefix: &Path) -> Result<Self> {
        let path = index_cache_path(prefix);
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading formula cache {}", path.display()))?;
        Self::parse_index(&contents)
    }

    fn fetch_remote_raw() -> Result<String> {
        let response = reqwest::blocking::get(FORMULA_API)
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
