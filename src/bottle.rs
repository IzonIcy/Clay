use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::fmt::Write;
use std::io;
use std::path::{Path, PathBuf};
use tar::Archive;

pub fn download(url: &str, dest: &Path) -> Result<()> {
    let mut response = reqwest::blocking::get(url)
        .with_context(|| format!("downloading bottle {url}"))?
        .error_for_status()
        .context("bottle download failed")?;
    let mut file = File::create(dest)
        .with_context(|| format!("creating bottle file {}", dest.display()))?;
    std::io::copy(&mut response, &mut file)
        .with_context(|| format!("writing bottle file {}", dest.display()))?;
    Ok(())
}

pub fn extract(tarball: &Path, dest: &Path) -> Result<()> {
    let file = File::open(tarball)
        .with_context(|| format!("opening bottle {}", tarball.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest)
        .with_context(|| format!("extracting bottle to {}", dest.display()))?;
    Ok(())
}

pub fn cache_path(cache_dir: &Path, formula: &str, version: &str) -> PathBuf {
    cache_dir.join(format!("{formula}-{version}.tar.gz"))
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let file = File::open(path)
        .with_context(|| format!("opening {} for checksum", path.display()))?;
    let actual = sha256_hex_reader(file)?;
    if actual != expected {
        anyhow::bail!(
            "checksum mismatch for {} (expected {}, got {})",
            path.display(),
            expected,
            actual
        );
    }
    Ok(())
}

fn sha256_hex_reader<R: io::Read>(mut reader: R) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let result = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in result {
        let _ = write!(out, "{:02x}", byte);
    }
    Ok(out)
}
