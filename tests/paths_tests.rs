// tests/paths_tests.rs
//
// Project-root anchoring (holtz #85).
//
// The bug these guard against: manifest keys were computed against the process
// cwd at five separate sites while the data_dir was resolved by walking up to
// the project root. A `cd` into a subdirectory therefore registered a second,
// differently-spelled entry for a file already tracked, and the short spelling
// resolved against the root — where nothing was — so it read as permanently
// missing and permanently tripped the `no_violations` gate.

use std::path::{Path, PathBuf};

use sahjhan::paths::{data_dir_from, is_managed, manifest_key, path_is_under, project_root_from};
use tempfile::tempdir;

/// Build `<tmp>/docs/holtz/.sahjhan` and hand back the root.
fn project_with_data_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/holtz/.sahjhan")).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// project_root_from
// ---------------------------------------------------------------------------

#[test]
fn test_project_root_is_the_ancestor_holding_data_dir() {
    let dir = project_with_data_dir();
    let root = dir.path();

    assert_eq!(project_root_from("docs/holtz/.sahjhan", root), root);
}

#[test]
fn test_project_root_is_the_same_from_any_subdirectory() {
    let dir = project_with_data_dir();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/deep/nest")).unwrap();

    // This is the reported reproduction: the shell drifts into the directory
    // the audit artifacts live in, which is exactly where an agent is tempted
    // to cd. The anchor must not move with it.
    for sub in ["docs", "docs/holtz", "docs/holtz/.sahjhan", "src/deep/nest"] {
        assert_eq!(
            project_root_from("docs/holtz/.sahjhan", &root.join(sub)),
            root,
            "project root moved when cwd was {}",
            sub
        );
    }
}

#[test]
fn test_project_root_falls_back_to_cwd_when_data_dir_absent() {
    // A fresh `init`: nothing holds the data_dir yet, so it is about to be
    // created relative to the caller. Preserves directory-creation semantics.
    let dir = tempdir().unwrap();
    assert_eq!(
        project_root_from("docs/holtz/.sahjhan", dir.path()),
        dir.path()
    );
}

#[test]
fn test_project_root_falls_back_to_cwd_for_absolute_data_dir() {
    // An absolute data_dir carries no anchor — it says nothing about where the
    // project starts.
    let dir = tempdir().unwrap();
    assert_eq!(
        project_root_from("/var/tmp/whatever", dir.path()),
        dir.path()
    );
}

// ---------------------------------------------------------------------------
// The invariant that keeps the two derivations from drifting
// ---------------------------------------------------------------------------

#[test]
fn test_data_dir_is_the_project_root_joined_with_data_dir() {
    let dir = project_with_data_dir();
    let root = dir.path();

    // #85 was two derivations of one fact disagreeing. data_dir_from is
    // *defined* as project_root_from(..).join(..), so they cannot: assert it
    // from every vantage point, including the one that produced the bug.
    for sub in [".", "docs", "docs/holtz", "docs/holtz/.sahjhan"] {
        let cwd = root.join(sub);
        assert_eq!(
            data_dir_from("docs/holtz/.sahjhan", &cwd),
            project_root_from("docs/holtz/.sahjhan", &cwd).join("docs/holtz/.sahjhan"),
            "derivations disagreed with cwd at {}",
            sub
        );
    }
}

#[test]
fn test_data_dir_from_subdirectory_still_finds_the_real_data_dir() {
    let dir = project_with_data_dir();
    let root = dir.path();

    assert_eq!(
        data_dir_from("docs/holtz/.sahjhan", &root.join("docs/holtz")),
        root.join("docs/holtz/.sahjhan")
    );
}

#[test]
fn test_absolute_data_dir_passes_through() {
    let dir = tempdir().unwrap();
    assert_eq!(
        data_dir_from("/abs/data", dir.path()),
        PathBuf::from("/abs/data")
    );
}

// ---------------------------------------------------------------------------
// manifest_key
// ---------------------------------------------------------------------------

#[test]
fn test_manifest_key_is_root_relative_regardless_of_cwd() {
    let dir = project_with_data_dir();
    let root = dir.path();
    let status = root.join("docs/holtz/STATUS.md");

    // Same file, keyed from the root and from inside docs/holtz — one spelling.
    let from_root = manifest_key(&status, &project_root_from("docs/holtz/.sahjhan", root));
    let from_sub = manifest_key(
        &status,
        &project_root_from("docs/holtz/.sahjhan", &root.join("docs/holtz")),
    );

    assert_eq!(from_root, "docs/holtz/STATUS.md");
    assert_eq!(from_root, from_sub);
}

#[test]
fn test_manifest_key_returns_path_as_written_when_outside_root() {
    // Not silently accepted downstream — Manifest::track refuses it (E13).
    let key = manifest_key(Path::new("/elsewhere/STATUS.md"), Path::new("/project"));
    assert_eq!(key, "/elsewhere/STATUS.md");
}

// ---------------------------------------------------------------------------
// path_is_under / is_managed
// ---------------------------------------------------------------------------

#[test]
fn test_path_is_under_matches_whole_components() {
    assert!(path_is_under("docs/holtz/STATUS.md", "docs/holtz"));
    assert!(path_is_under("docs/holtz", "docs/holtz"));

    // The latent bug in the old string `starts_with` check: a sibling
    // directory whose name merely begins with the managed one.
    assert!(!path_is_under("docs/holtz-old/STATUS.md", "docs/holtz"));
    assert!(!path_is_under("docs/holtzfoo", "docs/holtz"));

    // The phantom key from the report.
    assert!(!path_is_under("STATUS.md", "docs/holtz"));
    assert!(!path_is_under(".sahjhan/ledger.jsonl", "docs/holtz"));
}

#[test]
fn test_path_is_under_tolerates_trailing_slashes_and_dot_segments() {
    // `enforcement/` is how holtz spells it in protocol.toml.
    assert!(path_is_under("enforcement/hooks/x.py", "enforcement/"));
    assert!(path_is_under("./docs/holtz/STATUS.md", "docs/holtz"));
}

#[test]
fn test_path_is_under_refuses_parent_traversal() {
    // A key that climbs out of the tree it is spelled against cannot be
    // checked against anything, so it is never under a managed path.
    assert!(!path_is_under("../portfolio/STATUS.md", "docs/holtz"));
    assert!(!path_is_under("docs/holtz/../../etc/passwd", "docs/holtz"));
}

#[test]
fn test_empty_managed_paths_admits_everything() {
    // examples/lint-demo ships `managed = []`; a protocol that does not declare
    // what it manages cannot have that claim falsified.
    assert!(is_managed("anything/at/all.md", &[]));
}

#[test]
fn test_is_managed_accepts_any_declared_path() {
    let managed = vec!["docs/holtz".to_string(), "enforcement/".to_string()];
    assert!(is_managed("docs/holtz/PUNCHLIST.md", &managed));
    assert!(is_managed("enforcement/events.toml", &managed));
    assert!(!is_managed("STATUS.md", &managed));
}
