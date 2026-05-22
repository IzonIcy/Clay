use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const FORMULA_API: &str = "https://formulae.brew.sh/api/formula.json";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FormulaRecord {
    pub name: String,
    pub desc: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
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
}

#[derive(Debug)]
pub struct FormulaIndex {
    formulas: HashMap<String, FormulaRecord>,
}

impl FormulaIndex {
    pub fn fetch() -> Result<Self> {
        let response = reqwest::blocking::get(FORMULA_API)
            .context("fetching formula index")?
            .error_for_status()
            .context("formula API response error")?;
        let formulas: Vec<FormulaRecord> = response
            .json()
            .context("parsing formula index JSON")?;
        let mut map = HashMap::new();
        for formula in formulas {
            map.insert(formula.name.clone(), formula);
        }
        Ok(Self { formulas: map })
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
}
