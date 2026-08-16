//! End-to-end integration tests for Clay's install/link/cleanup flows.
//!
//! Everything runs against a temporary fake prefix — no network, no Homebrew.

use std::fs;
use std::path::{Path, PathBuf};

use clay::install::{
    autoremove, cleanup, is_installed, link_formula, list_installed_with_versions,
    uninstall_formula, unlink_formula,
};
use clay::prefix::{cache_dir, cellar, registry_path};
use clay::registry::{InstallRecord, Registry};

fn fake_prefix() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp prefix");
    let prefix = dir.path().to_path_buf();
    (dir, prefix)
}

fn make_cellar_file(prefix: &Path, formula: &str, version: &str, file: &str) -> PathBuf {
    let path = cellar(prefix).join(formula).join(version).join(file);
    fs::create_dir_all(path.parent().unwrap()).expect("create cellar dirs");
    fs::write(&path, "#!/bin/sh\necho fake\n").expect("write cellar file");
    path
}

fn install_record(name: &str, version: &str, requested: bool, deps: &[&str]) -> InstallRecord {
    InstallRecord {
        name: name.to_string(),
        version: version.to_string(),
        platform: "test".to_string(),
        installed_at: "2026-01-01T00:00:00Z".to_string(),
        links: Vec::new(),
        dependencies: deps.iter().map(|d| d.to_string()).collect(),
        requested,
    }
}

fn save_registry(prefix: &Path, records: Vec<InstallRecord>) {
    let mut registry = Registry::default();
    for record in records {
        registry.upsert(record);
    }
    registry
        .save(&registry_path(prefix))
        .expect("save registry");
}

#[test]
fn is_installed_detects_cellar_and_registry() {
    let (_dir, prefix) = fake_prefix();

    // Nothing installed yet.
    assert!(!is_installed(&prefix, "wget").expect("not installed"));

    // Detected via cellar directory.
    make_cellar_file(&prefix, "wget", "1.21", "bin/wget");
    assert!(is_installed(&prefix, "wget").expect("installed via cellar"));

    // Detected via registry even without a cellar entry.
    save_registry(&prefix, vec![install_record("jq", "1.7", true, &[])]);
    assert!(is_installed(&prefix, "jq").expect("installed via registry"));
}

#[test]
fn link_and_unlink_formula_roundtrip() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "wget", "1.21", "bin/wget");
    make_cellar_file(&prefix, "wget", "1.21", "lib/libwget.dylib");

    let linked = link_formula(&prefix, "wget", Some("1.21"), false).expect("link");
    assert_eq!(linked, 2, "both bin and lib entries should be linked");

    let bin_link = prefix.join("bin").join("wget");
    assert!(bin_link.is_symlink(), "bin/wget should be a symlink");
    assert!(
        fs::read_link(&bin_link)
            .unwrap()
            .to_string_lossy()
            .contains("Cellar/wget"),
        "symlink should point into the cellar"
    );

    // Registry now records the links; unlinking removes them.
    let unlinked = unlink_formula(&prefix, "wget", None).expect("unlink");
    assert_eq!(unlinked, 2);
    assert!(!bin_link.exists(), "bin/wget should be removed");

    // Unlinking twice removes nothing.
    assert_eq!(
        unlink_formula(&prefix, "wget", None).expect("unlink again"),
        0
    );
}

#[test]
fn link_conflicts_are_skipped_unless_overwrite() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "wget", "1.21", "bin/wget");

    // A real (non-symlink) file in the way blocks the link.
    let blocker = prefix.join("bin").join("wget");
    fs::create_dir_all(blocker.parent().unwrap()).unwrap();
    fs::write(&blocker, "user file").unwrap();

    let linked =
        link_formula(&prefix, "wget", Some("1.21"), false).expect("link without overwrite");
    assert_eq!(linked, 0, "conflicting file should not be linked");
    assert_eq!(fs::read_to_string(&blocker).unwrap(), "user file");

    // --overwrite replaces foreign symlinks, but never real user files.
    let linked = link_formula(&prefix, "wget", Some("1.21"), true).expect("link with overwrite");
    assert_eq!(linked, 0, "real files block linking even with --overwrite");
    assert_eq!(fs::read_to_string(&blocker).unwrap(), "user file");

    let foreign = prefix.join("bin").join("jq");
    std::os::unix::fs::symlink("/somewhere/else", &foreign).unwrap();
    make_cellar_file(&prefix, "jq", "1.7", "bin/jq");
    let linked = link_formula(&prefix, "jq", Some("1.7"), true).expect("link over foreign symlink");
    assert_eq!(linked, 1);
    assert!(foreign.is_symlink());
    assert!(
        fs::read_link(&foreign)
            .unwrap()
            .to_string_lossy()
            .contains("Cellar/jq"),
        "foreign symlink should be replaced with one into the cellar"
    );
}

#[test]
fn cleanup_keeps_latest_version_and_prunes_registry() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "wget", "1.20", "bin/wget");
    make_cellar_file(&prefix, "wget", "1.21", "bin/wget");
    save_registry(
        &prefix,
        vec![
            install_record("wget", "1.20", true, &[]),
            install_record("wget", "1.21", true, &[]),
        ],
    );

    let report = cleanup(&prefix).expect("cleanup");
    assert_eq!(report.removed_versions, 1);
    assert!(!cellar(&prefix).join("wget").join("1.20").exists());
    assert!(cellar(&prefix).join("wget").join("1.21").exists());

    // Registry no longer references the pruned version.
    let registry = Registry::load(&registry_path(&prefix)).expect("load registry");
    let versions: Vec<String> = registry
        .entries_for("wget")
        .into_iter()
        .map(|entry| entry.version)
        .collect();
    assert_eq!(versions, vec!["1.21"]);
}

#[test]
fn autoremove_removes_unrequested_leaves_but_keeps_requested() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "openssl@3", "3.0", "bin/openssl");
    make_cellar_file(&prefix, "wget", "1.21", "bin/wget");
    save_registry(
        &prefix,
        vec![
            install_record("openssl@3", "3.0", false, &[]),
            install_record("wget", "1.21", true, &[]),
        ],
    );

    let report = autoremove(&prefix).expect("autoremove");
    assert_eq!(report.removed_formulae, 1);
    assert!(!cellar(&prefix).join("openssl@3").exists());
    assert!(cellar(&prefix).join("wget").exists());
}

#[test]
fn uninstall_formula_is_blocked_by_dependents() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "openssl@3", "3.0", "bin/openssl");
    save_registry(
        &prefix,
        vec![
            install_record("openssl@3", "3.0", false, &[]),
            install_record("wget", "1.21", true, &["openssl@3"]),
        ],
    );

    let error = uninstall_formula("openssl@3", &prefix, false).expect_err("should be blocked");
    assert!(
        error.to_string().contains("still required by wget"),
        "unexpected error: {error}"
    );
}

#[test]
fn uninstall_formula_forces_with_ignore_dependencies() {
    let (_dir, prefix) = fake_prefix();
    let cellar_file = make_cellar_file(&prefix, "openssl@3", "3.0", "bin/openssl");
    save_registry(
        &prefix,
        vec![
            install_record("openssl@3", "3.0", false, &[]),
            install_record("wget", "1.21", true, &["openssl@3"]),
        ],
    );

    uninstall_formula("openssl@3", &prefix, true).expect("forced uninstall");
    assert!(!cellar_file.exists());
}

#[test]
fn list_installed_with_versions_reads_registry_and_cellar() {
    let (_dir, prefix) = fake_prefix();
    make_cellar_file(&prefix, "jq", "1.7", "bin/jq");

    // Without registry entries it falls back to scanning the cellar.
    let listed = list_installed_with_versions(&prefix).expect("list from cellar");
    assert_eq!(listed, vec![("jq".to_string(), vec!["1.7".to_string()])]);

    // Registry entries take precedence and merge versions.
    save_registry(
        &prefix,
        vec![
            install_record("jq", "1.6", true, &[]),
            install_record("jq", "1.7", true, &[]),
        ],
    );
    let listed = list_installed_with_versions(&prefix).expect("list from registry");
    assert_eq!(
        listed,
        vec![("jq".to_string(), vec!["1.6".to_string(), "1.7".to_string()])]
    );
}

#[test]
fn clean_cache_removes_files_but_not_directories() {
    let (_dir, prefix) = fake_prefix();
    let cache = cache_dir(&prefix);
    fs::create_dir_all(cache.join("subdir")).unwrap();
    fs::write(cache.join("formula.json"), "data").unwrap();
    fs::write(cache.join("subdir").join("bottle.tar.gz"), "data").unwrap();

    let removed = clay::cache::clean_cache(&prefix).expect("clean cache");
    assert_eq!(removed, 1, "only top-level files are removed");
    assert!(!cache.join("formula.json").exists());
    assert!(cache.join("subdir").join("bottle.tar.gz").exists());
}

#[test]
fn registry_roundtrips_through_disk() {
    let (_dir, prefix) = fake_prefix();
    let path = registry_path(&prefix);
    let mut registry = Registry::default();
    registry.upsert(install_record("wget", "1.21", true, &["openssl@3"]));
    registry.pin("wget");
    registry.save(&path).expect("save");

    let loaded = Registry::load(&path).expect("load");
    assert_eq!(loaded.installs.len(), 1);
    assert_eq!(loaded.installs[0].name, "wget");
    assert_eq!(loaded.installs[0].dependencies, vec!["openssl@3"]);
    assert!(loaded.is_pinned("wget"));

    // Loading a missing registry yields an empty one, not an error.
    let empty =
        Registry::load(&registry_path(&prefix).with_extension("nope")).expect("load missing");
    assert!(empty.installs.is_empty());
}
