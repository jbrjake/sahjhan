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
// project_root_from: linked git worktrees (#47)
//
// `data_dir` is run state, so it is gitignored, so no worktree ever contains
// one. A worktree that is not nested under the main checkout therefore has no
// ancestor holding it, and anchoring it on itself points every system-owned
// path at a directory nothing has written to.
//
// These build the on-disk shapes git produces rather than shelling out to it,
// so they are fast and stay readable about *which byte* the decision turns on;
// `test_sibling_worktree_reaches_the_projects_ledger` in integration_tests.rs
// runs the same case against real `git worktree add` to keep the fabrication
// honest.
// ---------------------------------------------------------------------------

/// A main checkout at `<tmp>/proj` — `.git` directory, `data_dir` present.
fn project_with_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("proj/docs/holtz/.sahjhan")).unwrap();
    std::fs::create_dir_all(dir.path().join("proj/.git")).unwrap();
    dir
}

/// Link `<tmp>/<at>` to `<tmp>/proj` the way `git worktree add` does: a `.git`
/// file pointing at an admin directory under the shared repository, and a
/// `commondir` in it naming that repository (relative, as git writes it).
fn link_worktree(tmp: &Path, at: &str, name: &str) -> PathBuf {
    let wt = tmp.join(at);
    std::fs::create_dir_all(&wt).unwrap();
    let admin = tmp.join("proj/.git/worktrees").join(name);
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", admin.to_string_lossy()),
    )
    .unwrap();
    wt
}

/// Both sides canonicalized — macOS spells the tempdir `/var/...` and its
/// realpath `/private/var/...`, and resolving `commondir` produces the latter.
fn assert_same_dir(got: PathBuf, want: &Path) {
    assert_eq!(
        got.canonicalize().unwrap(),
        want.canonicalize().unwrap(),
        "anchored on {:?}, wanted {:?}",
        got,
        want
    );
}

#[test]
fn test_sibling_worktree_anchors_on_the_main_checkout() {
    // The #47 reproduction: `git worktree add ../wt`, run a command from it.
    // Nothing above `<tmp>/wt` holds the data_dir, and before the fix that made
    // the worktree its own project — so every command resolved to a ledger
    // that does not exist and exited 2 without saying why.
    let dir = project_with_repo();
    let wt = link_worktree(dir.path(), "wt", "wt");

    assert_same_dir(
        project_root_from("docs/holtz/.sahjhan", &wt),
        &dir.path().join("proj"),
    );
}

#[test]
fn test_sibling_worktree_anchors_the_same_from_a_subdirectory_of_it() {
    // A fix agent works in `src/`, not at the worktree root.
    let dir = project_with_repo();
    let wt = link_worktree(dir.path(), "wt", "wt");
    std::fs::create_dir_all(wt.join("src/deep/nest")).unwrap();

    assert_same_dir(
        project_root_from("docs/holtz/.sahjhan", &wt.join("src/deep/nest")),
        &dir.path().join("proj"),
    );
}

#[test]
fn test_nested_worktree_still_anchors_by_walking_up() {
    // The layout Claude Code's `isolation: "worktree"` produces, and the case
    // that worked before the fix: the walk-up passes through the main checkout
    // and finds the data_dir, so no git resolution happens at all. Asserted
    // uncanonicalized on purpose — this path must return the ancestor exactly
    // as the caller spelled it.
    let dir = project_with_repo();
    let proj = dir.path().join("proj");
    std::fs::create_dir_all(proj.join(".worktrees")).unwrap();
    let wt = link_worktree(dir.path(), "proj/.worktrees/fix-1", "fix-1");

    assert_eq!(project_root_from("docs/holtz/.sahjhan", &wt), proj);
}

#[test]
fn test_submodule_does_not_anchor_on_its_superproject() {
    // The check that cost the consumer a separate bug to find. A submodule also
    // has a `.git` *file*, and its gitdir also lives under the superproject —
    // but it carries no `commondir`, because a submodule is its own repository
    // and deserves its own ledger. Following `gitdir:` without looking would
    // hand every submodule the superproject's state.
    let dir = project_with_repo();
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let admin = dir.path().join("proj/.git/modules/sub");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(
        sub.join(".git"),
        format!("gitdir: {}\n", admin.to_string_lossy()),
    )
    .unwrap();

    assert_eq!(project_root_from("docs/holtz/.sahjhan", &sub), sub);
}

#[test]
fn test_unresolvable_git_file_anchors_on_the_caller() {
    // Every unreadable shape leaves the caller exactly where it was before the
    // fix — this runs on the failure path of an anchor every command computes,
    // so it may not introduce a new way to fail.
    let dir = project_with_repo();
    let tmp = dir.path();

    let cases: [(&str, &str); 3] = [
        ("garbage", "not a gitdir pointer at all\n"),
        ("empty-pointer", "gitdir:\n"),
        ("dangling", "gitdir: /nonexistent/admin/dir\n"),
    ];
    for (name, content) in cases {
        let d = tmp.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".git"), content).unwrap();
        assert_eq!(
            project_root_from("docs/holtz/.sahjhan", &d),
            d,
            "{} should have anchored on itself",
            name
        );
    }
}

#[test]
fn test_directory_in_no_repository_at_all_anchors_on_itself() {
    // Unchanged, and stated separately from the fresh-`init` case above because
    // this is the one the worktree resolution must not disturb.
    let dir = tempdir().unwrap();
    let loose = dir.path().join("loose");
    std::fs::create_dir_all(&loose).unwrap();

    assert_eq!(project_root_from("docs/holtz/.sahjhan", &loose), loose);
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
