// src/manifest/verify.rs
//
// Manifest verification — compare files against recorded hashes.
//
// ## Index
// - Mismatch                 — file path + expected/actual hash
// - VerifyResult             — list of mismatches
// - [verify]                 verify()  — check all tracked files

use std::path::Path;

use super::tracker::{compute_file_sha256, Manifest};

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
}

/// Result of verifying the manifest against the filesystem.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// `true` if all tracked files match their recorded hashes.
    pub clean: bool,
    /// Details of any files that do not match.
    pub mismatches: Vec<Mismatch>,
}

// [verify]
/// Verify all entries in the manifest against files on disk.
///
/// For each tracked file, computes its SHA-256 and compares against the
/// recorded hash. Reports mismatches including deleted files.
///
/// `base_dir` is the project root directory; entry paths are resolved
/// relative to it.
pub fn verify(manifest: &Manifest, base_dir: &Path) -> VerifyResult {
    let mut mismatches = Vec::new();

    for (rel_path, entry) in &manifest.entries {
        let abs_path = base_dir.join(rel_path);

        if !abs_path.exists() {
            // File was deleted
            mismatches.push(Mismatch {
                path: rel_path.clone(),
                expected: entry.sha256.clone(),
                actual: None,
                last_operation: entry.last_operation.clone(),
                last_updated: entry.last_updated.clone(),
            });
            continue;
        }

        match compute_file_sha256(&abs_path) {
            Ok(actual_hash) => {
                if actual_hash != entry.sha256 {
                    mismatches.push(Mismatch {
                        path: rel_path.clone(),
                        expected: entry.sha256.clone(),
                        actual: Some(actual_hash),
                        last_operation: entry.last_operation.clone(),
                        last_updated: entry.last_updated.clone(),
                    });
                }
            }
            Err(_) => {
                // Cannot read file — treat as a mismatch with no actual hash
                mismatches.push(Mismatch {
                    path: rel_path.clone(),
                    expected: entry.sha256.clone(),
                    actual: None,
                    last_operation: entry.last_operation.clone(),
                    last_updated: entry.last_updated.clone(),
                });
            }
        }
    }

    // Sort mismatches by path for deterministic output
    mismatches.sort_by(|a, b| a.path.cmp(&b.path));

    VerifyResult {
        clean: mismatches.is_empty(),
        mismatches,
    }
}
