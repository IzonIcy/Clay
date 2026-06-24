use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use std::fmt::Write;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tar::{Archive, EntryType};

const USER_AGENT: &str = "clay/0.1 (+https://github.com)";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const DOWNLOAD_RETRIES: usize = 3;

pub fn download(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating cache directory {}", parent.display()))?;
    }

    let mut last_error = None;
    for attempt in 1..=DOWNLOAD_RETRIES {
        match download_once(url, dest) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                if attempt < DOWNLOAD_RETRIES {
                    std::thread::sleep(Duration::from_millis(250 * attempt as u64));
                }
            }
        }
    }

    Err(last_error.expect("download attempted at least once"))
}

fn download_once(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .context("building HTTP client")?;
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("downloading bottle {url}"))?
        .error_for_status()
        .context("bottle download failed")?;

    let tmp = dest.with_extension("tar.gz.part");
    let _ = std::fs::remove_file(&tmp);
    {
        let mut file = File::create(&tmp)
            .with_context(|| format!("creating bottle file {}", tmp.display()))?;
        std::io::copy(&mut response, &mut file)
            .with_context(|| format!("writing bottle file {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing bottle file {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, dest).with_context(|| {
        format!(
            "moving downloaded bottle {} to {}",
            tmp.display(),
            dest.display()
        )
    })?;
    Ok(())
}

pub fn extract(tarball: &Path, dest: &Path) -> Result<()> {
    let file =
        File::open(tarball).with_context(|| format!("opening bottle {}", tarball.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries().context("reading bottle archive")? {
        let mut entry = entry.context("reading bottle archive entry")?;
        let path = entry
            .path()
            .context("reading bottle archive entry path")?
            .into_owned();
        validate_archive_path(&path)?;

        let entry_type = entry.header().entry_type();
        if matches!(entry_type, EntryType::Symlink | EntryType::Link) {
            if let Some(link_name) = entry
                .link_name()
                .context("reading bottle archive link target")?
            {
                validate_archive_link_target(&path, &link_name)?;
            }
        }

        let unpacked = entry
            .unpack_in(dest)
            .with_context(|| format!("extracting {}", path.display()))?;
        if !unpacked {
            bail!(
                "refusing to extract path outside destination: {}",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn cache_path(cache_dir: &Path, formula: &str, version: &str) -> PathBuf {
    cache_dir.join(format!("{formula}-{version}.tar.gz"))
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let file =
        File::open(path).with_context(|| format!("opening {} for checksum", path.display()))?;
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

fn validate_archive_path(path: &Path) -> Result<()> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("unsafe path in bottle archive: {}", path.display());
    }
    Ok(())
}

fn validate_archive_link_target(entry_path: &Path, link_target: &Path) -> Result<()> {
    if link_target
        .components()
        .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    {
        bail!(
            "unsafe link target in bottle archive: {} -> {}",
            entry_path.display(),
            link_target.display()
        );
    }

    let base = entry_path.parent().unwrap_or_else(|| Path::new(""));
    let resolved = base.join(link_target);
    let mut depth = 0usize;
    for component in resolved.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    bail!(
                        "unsafe link target in bottle archive: {} -> {}",
                        entry_path.display(),
                        link_target.display()
                    );
                }
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "unsafe link target in bottle archive: {} -> {}",
                    entry_path.display(),
                    link_target.display()
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{sha256_hex_reader, validate_archive_link_target, validate_archive_path};
    use std::io::Cursor;
    use std::path::Path;

    #[test]
    fn computes_sha256_hex() {
        let hash = sha256_hex_reader(Cursor::new(b"hello".as_slice())).unwrap();
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn rejects_archive_paths_that_escape_destination() {
        assert!(validate_archive_path(Path::new("pkg/1.0/bin/tool")).is_ok());
        assert!(validate_archive_path(Path::new("../evil")).is_err());
        assert!(validate_archive_path(Path::new("pkg/../../evil")).is_err());
        assert!(validate_archive_path(Path::new("/tmp/evil")).is_err());
    }

    #[test]
    fn allows_relative_links_inside_archive_root_only() {
        assert!(validate_archive_link_target(
            Path::new("pkg/1.0/bin/tool"),
            Path::new("../lib/libtool.dylib")
        )
        .is_ok());
        assert!(
            validate_archive_link_target(Path::new("pkg/link"), Path::new("../../evil")).is_err()
        );
        assert!(
            validate_archive_link_target(Path::new("pkg/link"), Path::new("/tmp/evil")).is_err()
        );
    }
}
