// src/manifest/verify.rs
//
// Manifest verification — compare files against recorded hashes.
//
// ## Index
// - MismatchKind             — Modified / Missing / Unmanaged
// - Mismatch                 — file path + expected/actual hash + kind
// - VerifyResult             — mismatches plus unmanaged entries
// - [verify]                 verify()  — check all tracked files

use std::path::Path;

use super::tracker::{compute_file_sha256, Manifest};

/// Why an entry did not verify.
///
/// Before holtz #85 these were one undifferentiated "modified" count, so a
/// tampered file, a deleted file, and an entry for a path that was never real
/// produced the same words — and only the first of those is what the integrity
/// check exists to catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    /// The file exists and its contents changed since it was tracked.
    Modified,
    /// The file is gone, or can no longer be read.
    Missing,
    /// The entry's key is not under any managed path, so it cannot describe a
    /// file this manifest is responsible for. A bookkeeping defect, not
    /// tampering.
    Unmanaged,
}

impl MismatchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MismatchKind::Modified => "modified",
            MismatchKind::Missing => "missing",
            MismatchKind::Unmanaged => "unmanaged",
        }
    }
}

/// A file that does not match its recorded hash.
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    /// Relative path of the file.
    pub path: String,
    /// The SHA-256 hash recorded in the manifest.
    pub expected: String,
    /// The SHA-256 hash computed from disk, or `None` if the file was deleted.
    pub actual: Option<String>,
    /// The operation that last legitimately wrote this file.
    pub last_operation: String,
    /// ISO 8601 timestamp of the last legitimate write.
    pub last_updated: String,
    /// What kind of failure this is.
    pub kind: MismatchKind,
}

/// Result of verifying the manifest against the filesystem.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// `true` if every managed entry matches its recorded hash.
    ///
    /// Unmanaged entries do not affect this: an entry outside `managed_paths`
    /// makes no claim about a managed file, so it cannot falsify one.
    pub clean: bool,
    /// Managed files that are modified or missing.
    pub mismatches: Vec<Mismatch>,
    /// Entries whose key is outside `managed_paths` — reported so they stay
    /// visible, but not counted as integrity failures.
    pub unmanaged: Vec<Mismatch>,
}

// [verify]
/// Verify all entries in the manifest against files on disk.
///
/// For each tracked file, computes its SHA-256 and compares against the
/// recorded hash. Reports mismatches including deleted files.
///
/// `base_dir` is the **project root** — the directory manifest keys are spelled
/// against. Passing the process cwd instead is the read-side half of holtz #85:
/// verified from a subdirectory, every correctly-keyed entry reads as missing.
pub fn verify(manifest: &Manifest, base_dir: &Path) -> VerifyResult {
    let mut mismatches = Vec::new();
    let mut unmanaged = Vec::new();

    for (rel_path, entry) in &manifest.entries {
        let mismatch = |kind, actual| Mismatch {
            path: rel_path.clone(),
            expected: entry.sha256.clone(),
            actual,
            last_operation: entry.last_operation.clone(),
            last_updated: entry.last_updated.clone(),
            kind,
        };

        if !crate::paths::is_managed(rel_path, &manifest.managed_paths) {
            unmanaged.push(mismatch(MismatchKind::Unmanaged, None));
            continue;
        }

        let abs_path = base_dir.join(rel_path);

        if !abs_path.exists() {
            mismatches.push(mismatch(MismatchKind::Missing, None));
            continue;
        }

        match compute_file_sha256(&abs_path) {
            Ok(actual_hash) => {
                if actual_hash != entry.sha256 {
                    mismatches.push(mismatch(MismatchKind::Modified, Some(actual_hash)));
                }
            }
            // Present but unreadable — the recorded contents cannot be
            // confirmed, which is the same standing as gone.
            Err(_) => mismatches.push(mismatch(MismatchKind::Missing, None)),
        }
    }

    // Sort by path for deterministic output
    mismatches.sort_by(|a, b| a.path.cmp(&b.path));
    unmanaged.sort_by(|a, b| a.path.cmp(&b.path));

    VerifyResult {
        clean: mismatches.is_empty(),
        mismatches,
        unmanaged,
    }
}
