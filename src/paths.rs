// src/paths.rs
//
// Project-root anchoring for system-owned paths.
//
// The rule: **paths the system owns are resolved against the project root;
// paths a user types on the command line stay relative to the current
// directory.** System-owned means `data_dir`, `render_dir`, manifest keys,
// the manifest verify base, and the working directory a gate command runs in.
// User-typed means `--config-dir`, `ledger create --path`, `ledger import`,
// which keep ordinary UNIX semantics.
//
// Every manifest key is spelled relative to the project root, so the same file
// has the same key no matter which directory a command was invoked from. Before
// holtz #85 the key was computed against the process cwd at five separate call
// sites, so a `cd` into a subdirectory registered a second, differently-spelled
// entry for a file already tracked — and the short spelling resolved against
// the root, where nothing was, so it read as permanently missing.
//
// The anchor understands linked git worktrees (#47), because it has to: the
// directory it looks for is gitignored, so a worktree never contains one and a
// walk that only goes up would answer "this worktree is its own project" for
// every actor working outside the main checkout.
//
// ## Index
// - [project-root]     project_root_from()    — ancestor of cwd that holds data_dir
// - [walk-up]          walk_up_for()          — the walk itself, or None
// - [linked-worktree]  linked_worktree_main() — main worktree behind a `.git` file
// - [git-common-dir]   main_worktree_of()     — `gitdir:` → `commondir` → main worktree
// - [data-dir]         data_dir_from()        — project_root_from() joined with data_dir
// - [manifest-key]     manifest_key()         — project-root-relative key for a file
// - [path-under]       path_is_under()        — component-wise containment test
// - [is-managed]       is_managed()           — path under any of managed_paths

use std::path::{Path, PathBuf};

// [project-root]
/// Resolve the project root — the anchor every system-owned path hangs off.
///
/// A relative `data_dir` (e.g. `docs/holtz/.sahjhan`) names its own anchor:
/// the project root is the nearest ancestor of `cwd` that already contains it.
/// That is what makes a `cd` into a subdirectory harmless — the same ancestor
/// is found from anywhere inside the tree.
///
/// A **linked worktree** is the one place that walk answers wrongly rather than
/// inconclusively (#47). `data_dir` holds run state, so it is gitignored, so
/// `git worktree add` never produces it — and a worktree that is not nested
/// under the main checkout therefore has no ancestor holding it. Anchoring such
/// a caller on itself resolves every system-owned path to a directory nothing
/// has ever written to: same repository, same config, same `--config-dir`, and
/// every command exits 2 saying only that a file is missing. So before giving
/// up, resolve the worktree to its main checkout and walk up from *there*.
///
/// Falls back to `cwd` when `data_dir` is absolute (it carries no anchor), or
/// when neither walk finds it (a fresh `init`, where the directory is about to
/// be created relative to cwd).
pub fn project_root_from(data_dir: &str, cwd: &Path) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        return cwd.to_path_buf();
    }
    if let Some(root) = walk_up_for(&p, cwd) {
        return root;
    }
    if let Some(main) = linked_worktree_main(cwd) {
        if let Some(root) = walk_up_for(&p, &main) {
            return root;
        }
    }
    cwd.to_path_buf()
}

// [walk-up]
/// The nearest ancestor of `start` (inclusive) containing `rel`, if any.
///
/// Split out so the ordinary walk and the retry from a main worktree are the
/// same walk rather than two spellings of it — the failure mode #85 was.
fn walk_up_for(rel: &Path, start: &Path) -> Option<PathBuf> {
    let mut dir: Option<&Path> = Some(start);
    while let Some(d) = dir {
        if d.join(rel).exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

// [linked-worktree]
/// The main worktree of the linked worktree holding `cwd`, if it is in one.
///
/// Walks up to the nearest `.git`, which is the repository boundary. A `.git`
/// *directory* means this is already an ordinary checkout — the walk above has
/// been through it and found nothing, so there is nothing further to try.
///
/// Filesystem reads only, deliberately: this runs on the failure path of an
/// anchor every command computes, including `hook eval` on every tool call, so
/// it must not fork a `git` process. It must not fail loudly either — every
/// unreadable case returns `None` and leaves the caller anchored on itself,
/// which is exactly the behaviour that existed before.
fn linked_worktree_main(cwd: &Path) -> Option<PathBuf> {
    let mut dir: Option<&Path> = Some(cwd);
    while let Some(d) = dir {
        let git = d.join(".git");
        if git.is_dir() {
            return None;
        }
        if git.is_file() {
            return main_worktree_of(&git);
        }
        dir = d.parent();
    }
    None
}

// [git-common-dir]
/// Resolve a `.git` *file* to the main worktree it belongs to, or `None`.
///
/// Two different things put a file where `.git` would normally be a directory,
/// and only one of them is part of this project. A linked worktree's gitdir
/// carries a `commondir` naming the shared repository; a submodule's does not,
/// because a submodule *is* its own repository and deserves its own ledger.
/// Following the `gitdir:` pointer without that check would silently merge
/// every submodule into its superproject — which is a different bug, and a
/// quieter one, than the one being fixed.
fn main_worktree_of(git_link: &Path) -> Option<PathBuf> {
    let pointer = std::fs::read_to_string(git_link).ok()?;
    let gitdir = pointer.split_once("gitdir:")?.1.lines().next()?.trim();
    if gitdir.is_empty() {
        return None;
    }
    let gitdir = resolve_against(Path::new(gitdir), git_link.parent()?);

    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = common.trim();
    if common.is_empty() {
        return None;
    }
    // `commondir` is written relative to the gitdir (`../..`), so it has to be
    // canonicalized before a parent means anything.
    let common = resolve_against(Path::new(common), &gitdir)
        .canonicalize()
        .ok()?;
    // `commondir` names the shared `.git`; the main worktree contains it.
    common.parent().map(Path::to_path_buf)
}

/// `p` as written when absolute, else joined onto `base`.
fn resolve_against(p: &Path, base: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

// [data-dir]
/// Resolve `data_dir` against the project root.
///
/// Defined *in terms of* [`project_root_from`] rather than repeating the
/// walk-up, so the directory the ledger lives in and the anchor manifest keys
/// are spelled against can never disagree.
pub fn data_dir_from(data_dir: &str, cwd: &Path) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        return p;
    }
    project_root_from(data_dir, cwd).join(p)
}

// [manifest-key]
/// Compute the manifest key for a file: its path relative to the project root.
///
/// When `path` is not under `project_root` the path is returned as written.
/// That case is not silently accepted downstream — `Manifest::track` refuses
/// any key that is not under a managed path (E13).
pub fn manifest_key(path: &Path, project_root: &Path) -> String {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

// [path-under]
/// Test whether `path` is inside `managed`, comparing whole path components.
///
/// String `starts_with` is wrong here: it puts `docs/holtz-old/x` under
/// `docs/holtz`. Both sides tolerate trailing slashes and `./` segments, since
/// `managed` is author-written config (`enforcement/` is spelled with one).
///
/// A `..` component is refused outright — a key that climbs out of the tree it
/// is spelled against cannot be checked against anything.
pub fn path_is_under(path: &str, managed: &str) -> bool {
    let target = components(path);
    if target.contains(&"..") {
        return false;
    }
    let prefix = components(managed);
    // An empty prefix (`""`, `"."`, `"/"`) means the whole project.
    target.len() >= prefix.len() && target[..prefix.len()] == prefix[..]
}

// [is-managed]
/// Test whether `path` is under any of `managed_paths`.
///
/// An empty `managed_paths` declares no constraint and admits everything —
/// `examples/lint-demo` ships `managed = []`, and a protocol that does not
/// declare what it manages cannot have that claim falsified.
pub fn is_managed(path: &str, managed_paths: &[String]) -> bool {
    managed_paths.is_empty() || managed_paths.iter().any(|m| path_is_under(path, m))
}

/// Split a path into non-empty, non-`.` components.
fn components(p: &str) -> Vec<&str> {
    p.split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect()
}
