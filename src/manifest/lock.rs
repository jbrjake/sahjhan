// src/manifest/lock.rs
//
// The manifest lock: what makes a manifest load, the writes it seals, and the
// save one step to every other process (holtz#ho-0mvwg8q).
//
// ## Index
// - [manifest-lock]       ManifestLock::exclusive() — writer: held from manifest load to last save
// - [manifest-read-lock]  ManifestReadLock::shared() — reader: `manifest verify`'s consistent snapshot
// - ManifestGuard         — what holding either lock lets a caller name: its data dir
// - LOCK_TIMEOUT_SECS     — how long a writer or reader waits before refusing

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;

/// How long to wait for the lock before refusing.
///
/// A writer holds it across a ledger append, a manifest save and any renders
/// the event triggers — never across a gate or emit command, which may itself
/// run `sahjhan event` and would wait on its own parent. So the wait is a
/// queue of short critical sections, and this is sized for a burst of
/// parallel writers on a large ledger, not for one slow one.
pub const LOCK_TIMEOUT_SECS: u64 = 30;

/// Holding a manifest lock, of either kind, for one data dir.
///
/// The CLI's manifest helpers take this rather than a path, so a manifest can
/// only be loaded or saved by code that holds the lock for the directory it
/// names.
pub trait ManifestGuard {
    fn data_dir(&self) -> &Path;
}

// [manifest-lock]
/// Exclusive hold on a data dir's manifest.
///
/// The lock is an `flock` on the data dir itself rather than on a lock file
/// beside the manifest, so taking it creates nothing: a refused `init` still
/// writes nothing, and no consumer's data dir grows a file it has to ignore.
/// `Manifest::save` replaces `manifest.json` by rename, so a lock on that file
/// would be a lock on an inode the next save unlinks.
///
/// Released on drop.
#[derive(Debug)]
pub struct ManifestLock {
    _dir: File,
    data_dir: PathBuf,
}

impl ManifestLock {
    /// Wait up to [`LOCK_TIMEOUT_SECS`] for the exclusive lock on `data_dir`.
    pub fn exclusive(data_dir: &Path) -> Result<Self, String> {
        let dir = acquire(data_dir, Mode::Exclusive)?;
        Ok(ManifestLock {
            _dir: dir,
            data_dir: data_dir.to_path_buf(),
        })
    }
}

impl ManifestGuard for ManifestLock {
    fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

// [manifest-read-lock]
/// Shared hold on a data dir's manifest: no writer is between a load and its
/// last save while this is held, so a verify reads the manifest and the files
/// it seals as one state. Readers do not block each other.
///
/// Released on drop.
#[derive(Debug)]
pub struct ManifestReadLock {
    _dir: File,
    data_dir: PathBuf,
}

impl ManifestReadLock {
    /// Wait up to [`LOCK_TIMEOUT_SECS`] for a shared lock on `data_dir`.
    pub fn shared(data_dir: &Path) -> Result<Self, String> {
        let dir = acquire(data_dir, Mode::Shared)?;
        Ok(ManifestReadLock {
            _dir: dir,
            data_dir: data_dir.to_path_buf(),
        })
    }
}

impl ManifestGuard for ManifestReadLock {
    fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Shared,
    Exclusive,
}

fn acquire(data_dir: &Path, mode: Mode) -> Result<File, String> {
    let dir = File::open(data_dir)
        .map_err(|e| format!("cannot open {} to lock it: {}", data_dir.display(), e))?;
    let deadline = Instant::now() + Duration::from_secs(LOCK_TIMEOUT_SECS);
    loop {
        // fs2's, named: std's `File::try_lock_shared` (Rust 1.89) is an
        // inherent method, shadows the trait's, and errs with another type.
        let attempt = match mode {
            Mode::Shared => FileExt::try_lock_shared(&dir),
            Mode::Exclusive => FileExt::try_lock_exclusive(&dir),
        };
        match attempt {
            Ok(()) => return Ok(dir),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "timed out after {}s waiting for the manifest lock on {}",
                        LOCK_TIMEOUT_SECS,
                        data_dir.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                return Err(format!("cannot lock {}: {}", data_dir.display(), e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_excludes_both_kinds_and_shared_admits_shared() {
        let dir = tempfile::tempdir().unwrap();
        let held = ManifestLock::exclusive(dir.path()).unwrap();

        let probe = File::open(dir.path()).unwrap();
        let shared = |f: &File| FileExt::try_lock_shared(f).is_ok();
        let exclusive = |f: &File| FileExt::try_lock_exclusive(f).is_ok();
        assert!(!shared(&probe), "exclusive admitted a reader");
        assert!(!exclusive(&probe), "exclusive admitted a writer");
        drop(held);
        assert!(exclusive(&probe), "drop did not release");
        FileExt::unlock(&probe).unwrap();

        let r1 = ManifestReadLock::shared(dir.path()).unwrap();
        let r2 = ManifestReadLock::shared(dir.path()).unwrap();
        assert!(!exclusive(&probe), "a reader admitted a writer");
        drop((r1, r2));
        assert!(exclusive(&probe));
    }

    #[test]
    fn a_missing_data_dir_is_an_error_not_a_lock() {
        let dir = tempfile::tempdir().unwrap();
        let err = ManifestLock::exclusive(&dir.path().join("absent")).unwrap_err();
        assert!(err.contains("cannot open"), "{}", err);
    }
}
