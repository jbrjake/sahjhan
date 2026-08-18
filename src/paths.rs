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
// ## Index
// - [project-root]  project_root_from()  — ancestor of cwd that holds data_dir
// - [data-dir]      data_dir_from()      — project_root_from() joined with data_dir
// - [manifest-key]  manifest_key()       — project-root-relative key for a file
// - [path-under]    path_is_under()      — component-wise containment test
// - [is-managed]    is_managed()         — path under any of managed_paths

use std::path::{Path, PathBuf};

// [project-root]
/// Resolve the project root — the anchor every system-owned path hangs off.
///
/// A relative `data_dir` (e.g. `docs/holtz/.sahjhan`) names its own anchor:
/// the project root is the nearest ancestor of `cwd` that already contains it.
/// That is what makes a `cd` into a subdirectory harmless — the same ancestor
/// is found from anywhere inside the tree.
///
/// Falls back to `cwd` when `data_dir` is absolute (it carries no anchor) or
/// when no ancestor holds it (a fresh `init`, where the directory is about to
/// be created relative to cwd).
pub fn project_root_from(data_dir: &str, cwd: &Path) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        return cwd.to_path_buf();
    }
    let mut dir: Option<&Path> = Some(cwd);
    while let Some(d) = dir {
        if d.join(&p).exists() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }
    cwd.to_path_buf()
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
