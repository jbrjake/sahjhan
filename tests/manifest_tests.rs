use std::fs;

use sahjhan::manifest::tracker::{Manifest, RestoreAction};
use sahjhan::manifest::verify;
use sahjhan::manifest::verify::MismatchKind;
use tempfile::tempdir;

#[test]
fn test_init_creates_manifest() {
    let managed = vec!["docs/holtz".to_string()];
    let manifest = Manifest::init("docs/holtz/.sahjhan", managed.clone()).unwrap();

    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.managed_paths, managed);
    assert!(manifest.entries.is_empty());
    assert!(!manifest.manifest_hash.is_empty());

    // Save and reload to verify JSON structure
    let dir = tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let mut m = manifest;
    m.save(&path).unwrap();

    let loaded = Manifest::load(&path).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.managed_paths.len(), 1);
    assert!(loaded.entries.is_empty());
    assert_eq!(loaded.manifest_hash, m.manifest_hash);
}

#[test]
fn test_track_file_updates_hash() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    // Create a file to track
    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "# Punchlist\n- Item 1\n").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();

    manifest
        .track(
            "docs/holtz/PUNCHLIST.md",
            &file_path,
            "event finding --id BH-001",
            14,
        )
        .unwrap();

    let entry = manifest.entries.get("docs/holtz/PUNCHLIST.md").unwrap();
    assert_eq!(entry.sha256.len(), 64); // SHA-256 hex is 64 chars
    assert_eq!(entry.last_operation, "event finding --id BH-001");
    assert_eq!(entry.ledger_seq, 14);
    assert!(!entry.last_updated.is_empty());
}

#[test]
fn test_verify_detects_modification() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "original content").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "init", 1)
        .unwrap();

    // Tamper with the file
    fs::write(&file_path, "tampered content").unwrap();

    let result = verify::verify(&manifest, base);
    assert!(!result.clean);
    assert_eq!(result.mismatches.len(), 1);

    let mismatch = &result.mismatches[0];
    assert_eq!(mismatch.path, "docs/holtz/PUNCHLIST.md");
    assert!(mismatch.actual.is_some());
    assert_ne!(mismatch.actual.as_ref().unwrap(), &mismatch.expected);
    assert_eq!(mismatch.last_operation, "init");
}

#[test]
fn test_verify_passes_clean() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "clean content").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "init", 1)
        .unwrap();

    let result = verify::verify(&manifest, base);
    assert!(result.clean);
    assert!(result.mismatches.is_empty());
}

#[test]
fn test_track_updates_existing() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "version 1").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "op1", 1)
        .unwrap();
    let hash1 = manifest
        .entries
        .get("docs/holtz/PUNCHLIST.md")
        .unwrap()
        .sha256
        .clone();

    // Update the file and re-track
    fs::write(&file_path, "version 2").unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "op2", 5)
        .unwrap();

    let entry = manifest.entries.get("docs/holtz/PUNCHLIST.md").unwrap();
    assert_ne!(entry.sha256, hash1);
    assert_eq!(entry.last_operation, "op2");
    assert_eq!(entry.ledger_seq, 5);
}

#[test]
fn test_manifest_hash_changes_on_update() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "content A").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    let hash_empty = manifest.manifest_hash.clone();

    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "op1", 1)
        .unwrap();
    let hash_after_track = manifest.manifest_hash.clone();
    assert_ne!(hash_empty, hash_after_track);

    // Update file and re-track
    fs::write(&file_path, "content B").unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "op2", 2)
        .unwrap();
    let hash_after_update = manifest.manifest_hash.clone();
    assert_ne!(hash_after_track, hash_after_update);
}

#[test]
fn test_data_dir_must_be_under_managed_path() {
    // E12: data_dir outside managed paths should fail
    let result = Manifest::init("/tmp/outside/.sahjhan", vec!["docs/holtz".to_string()]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("E12"));

    // data_dir under a managed path should succeed
    let result = Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]);
    assert!(result.is_ok());

    // data_dir that IS the managed path should succeed
    let result = Manifest::init("docs/holtz", vec!["docs/holtz".to_string()]);
    assert!(result.is_ok());
}

#[test]
fn test_verify_handles_deleted_file() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("PUNCHLIST.md");
    fs::write(&file_path, "will be deleted").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/PUNCHLIST.md", &file_path, "init", 1)
        .unwrap();

    // Delete the file
    fs::remove_file(&file_path).unwrap();

    let result = verify::verify(&manifest, base);
    assert!(!result.clean);
    assert_eq!(result.mismatches.len(), 1);

    let mismatch = &result.mismatches[0];
    assert_eq!(mismatch.path, "docs/holtz/PUNCHLIST.md");
    assert!(mismatch.actual.is_none()); // None indicates deleted
    assert!(!mismatch.expected.is_empty());
    // A deleted managed file is "missing", not "modified" — the report noted
    // that verify called the same thing both at once (holtz #85 fix 3).
    assert_eq!(mismatch.kind, MismatchKind::Missing);
}

// ---------------------------------------------------------------------------
// holtz #85 — the manifest refuses keys it cannot be responsible for
// ---------------------------------------------------------------------------

#[test]
fn test_track_refuses_a_key_outside_managed_paths() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("STATUS.md");
    fs::write(&file_path, "# Status\n").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();

    // This is the phantom key from the report: the same real file, spelled as
    // if the project root were docs/holtz. It resolves against the actual root
    // to nothing, so admitting it means permanent, unrecoverable "missing".
    let err = manifest
        .track("STATUS.md", &file_path, "render", 1)
        .unwrap_err();
    assert!(err.contains("E13"), "unexpected error: {}", err);
    assert!(manifest.entries.is_empty(), "phantom key was registered");

    // The correctly-anchored key for the identical file is accepted.
    manifest
        .track("docs/holtz/STATUS.md", &file_path, "render", 1)
        .unwrap();
    assert_eq!(manifest.entries.len(), 1);
}

#[test]
fn test_track_allows_anything_when_managed_paths_is_empty() {
    // examples/lint-demo ships `managed = []`. No declaration, no constraint.
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("anywhere.md");
    fs::write(&file_path, "x").unwrap();

    let mut manifest = Manifest {
        version: 1,
        managed_paths: vec![],
        entries: Default::default(),
        manifest_hash: String::new(),
    };
    manifest
        .track("anywhere.md", &file_path, "render", 1)
        .unwrap();
    assert_eq!(manifest.entries.len(), 1);
}

#[test]
fn test_e12_uses_component_boundaries() {
    // `docs/holtz-old` is not under `docs/holtz`, though the string starts
    // with it — the check the old raw `starts_with` got wrong.
    let result = Manifest::init("docs/holtz-old/.sahjhan", vec!["docs/holtz".to_string()]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("E12"));
}

#[test]
fn test_verify_reports_unmanaged_entries_without_failing() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("STATUS.md");
    fs::write(&file_path, "# Status\n").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/STATUS.md", &file_path, "render", 1)
        .unwrap();

    // Simulate a manifest already poisoned by the pre-fix key derivation.
    // track() now refuses this, so it can only arrive from an older binary —
    // which is exactly the upgrade case that has to stop bleeding.
    manifest.entries.insert(
        "STATUS.md".to_string(),
        manifest.entries["docs/holtz/STATUS.md"].clone(),
    );

    let result = verify::verify(&manifest, base);
    assert!(
        result.clean,
        "an entry that cannot describe a managed file must not read as tampering"
    );
    assert!(result.mismatches.is_empty());
    assert_eq!(result.unmanaged.len(), 1);
    assert_eq!(result.unmanaged[0].path, "STATUS.md");
    assert_eq!(result.unmanaged[0].kind, MismatchKind::Unmanaged);
}

#[test]
fn test_verify_still_fails_on_a_real_modification_alongside_unmanaged_entries() {
    // The forgiveness above is scoped to keys outside managed_paths. A managed
    // file that actually changed must still fail, poisoned manifest or not.
    let dir = tempdir().unwrap();
    let base = dir.path();
    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("STATUS.md");
    fs::write(&file_path, "# Status\n").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/STATUS.md", &file_path, "render", 1)
        .unwrap();
    manifest.entries.insert(
        "STATUS.md".to_string(),
        manifest.entries["docs/holtz/STATUS.md"].clone(),
    );

    fs::write(
        &file_path,
        "# Status\n\nedited behind the protocol's back\n",
    )
    .unwrap();

    let result = verify::verify(&manifest, base);
    assert!(!result.clean);
    assert_eq!(result.mismatches.len(), 1);
    assert_eq!(result.mismatches[0].path, "docs/holtz/STATUS.md");
    assert_eq!(result.mismatches[0].kind, MismatchKind::Modified);
    assert!(result.mismatches[0].actual.is_some());
    assert_eq!(result.unmanaged.len(), 1);
}

#[test]
fn test_restore_placeholder() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    let file_dir = base.join("docs/holtz");
    fs::create_dir_all(&file_dir).unwrap();

    // Track a rendered file
    let rendered = file_dir.join("rendered.md");
    fs::write(&rendered, "rendered content").unwrap();

    // Track an agent-authored file
    let authored = file_dir.join("authored.md");
    fs::write(&authored, "authored content").unwrap();

    let mut manifest =
        Manifest::init("docs/holtz/.sahjhan", vec!["docs/holtz".to_string()]).unwrap();
    manifest
        .track("docs/holtz/rendered.md", &rendered, "render punchlist", 10)
        .unwrap();
    manifest
        .track(
            "docs/holtz/authored.md",
            &authored,
            "event finding --id BH-001",
            12,
        )
        .unwrap();

    // Rendered file should get ReRender instruction
    let action = manifest.restore_instruction("docs/holtz/rendered.md");
    assert_eq!(
        action,
        RestoreAction::ReRender {
            path: "docs/holtz/rendered.md".to_string(),
            ledger_seq: 10,
        }
    );

    // Agent-authored file should get GitCheckout instruction
    let action = manifest.restore_instruction("docs/holtz/authored.md");
    assert_eq!(
        action,
        RestoreAction::GitCheckout {
            path: "docs/holtz/authored.md".to_string(),
        }
    );

    // Untracked file should get NotTracked
    let action = manifest.restore_instruction("nonexistent.md");
    assert_eq!(
        action,
        RestoreAction::NotTracked {
            path: "nonexistent.md".to_string(),
        }
    );
}
