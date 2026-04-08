use sahjhan::daemon::auth::TrustedCallersManifest;
use tempfile::tempdir;

#[test]
fn test_load_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("trusted-callers.toml");
    std::fs::write(
        &manifest_path,
        r#"[callers]
"hooks/pre_tool.py" = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
"hooks/stop.py" = "sha256:abc123"
"#,
    )
    .unwrap();

    let manifest = TrustedCallersManifest::load(&manifest_path).unwrap();
    assert_eq!(manifest.callers.len(), 2);
    assert!(manifest.callers.contains_key("hooks/pre_tool.py"));
    assert!(manifest.callers.contains_key("hooks/stop.py"));
}

#[test]
fn test_load_manifest_missing_file() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("nonexistent.toml");
    let result = TrustedCallersManifest::load(&manifest_path);
    assert!(result.is_err());
}

#[test]
fn test_verify_script_hash_match() {
    let dir = tempdir().unwrap();

    let script_path = dir.path().join("hooks").join("test.py");
    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(&script_path, "print('hello')\n").unwrap();

    use sha2::{Digest, Sha256};
    let content = std::fs::read(&script_path).unwrap();
    let hash = format!("sha256:{}", hex::encode(Sha256::digest(&content)));

    let manifest_path = dir.path().join("trusted-callers.toml");
    std::fs::write(
        &manifest_path,
        format!("[callers]\n\"hooks/test.py\" = \"{}\"\n", hash),
    )
    .unwrap();

    let manifest = TrustedCallersManifest::load(&manifest_path).unwrap();
    let result = manifest.verify_caller(dir.path(), "hooks/test.py");
    assert!(result.is_ok());
}

#[test]
fn test_verify_script_hash_mismatch() {
    let dir = tempdir().unwrap();

    let script_path = dir.path().join("hooks").join("test.py");
    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(&script_path, "print('hello')\n").unwrap();

    let manifest_path = dir.path().join("trusted-callers.toml");
    std::fs::write(
        &manifest_path,
        "[callers]\n\"hooks/test.py\" = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n",
    )
    .unwrap();

    let manifest = TrustedCallersManifest::load(&manifest_path).unwrap();
    let result = manifest.verify_caller(dir.path(), "hooks/test.py");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("hash mismatch"), "got: {}", err_msg);
}

#[test]
fn test_verify_script_not_in_manifest() {
    let dir = tempdir().unwrap();

    let manifest_path = dir.path().join("trusted-callers.toml");
    std::fs::write(&manifest_path, "[callers]\n").unwrap();

    let manifest = TrustedCallersManifest::load(&manifest_path).unwrap();
    let result = manifest.verify_caller(dir.path(), "hooks/unknown.py");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("not in manifest"), "got: {}", err_msg);
}

#[test]
fn test_extract_script_path_from_cmdline() {
    use sahjhan::daemon::auth::extract_script_path;

    let args = vec![
        "/usr/bin/python3".to_string(),
        "/path/to/script.py".to_string(),
        "--flag".to_string(),
        "value".to_string(),
    ];
    assert_eq!(
        extract_script_path(&args),
        Some("/path/to/script.py".to_string())
    );

    let args = vec![
        "/usr/bin/python3".to_string(),
        "-u".to_string(),
        "/path/to/script.py".to_string(),
    ];
    assert_eq!(
        extract_script_path(&args),
        Some("/path/to/script.py".to_string())
    );

    let args = vec!["/bin/bash".to_string()];
    assert_eq!(extract_script_path(&args), None);

    let args: Vec<String> = vec![];
    assert_eq!(extract_script_path(&args), None);
}

// ---------------------------------------------------------------------------
// AuthError → reason code mapping (#26)
// ---------------------------------------------------------------------------

#[test]
fn test_auth_error_reason_codes() {
    use sahjhan::daemon::auth::AuthError;

    // Each AuthError variant should map to a known reason code
    let cases: Vec<(AuthError, &str)> = vec![
        (AuthError::NoScriptPath, "pid_resolution_failed"),
        (
            AuthError::Platform("test".to_string()),
            "pid_resolution_failed",
        ),
        (
            AuthError::HashMismatch {
                path: "test.py".to_string(),
                expected: "sha256:aaa".to_string(),
                actual: "sha256:bbb".to_string(),
            },
            "hash_mismatch",
        ),
        (
            AuthError::NotInManifest {
                path: "unknown.py".to_string(),
            },
            "pid_resolution_failed",
        ),
    ];

    for (error, expected_reason) in cases {
        assert_eq!(
            error.reason_code(),
            expected_reason,
            "AuthError::{:?} should map to '{}'",
            error,
            expected_reason
        );
    }
}

// ---------------------------------------------------------------------------
// Process tree walking (#26)
// ---------------------------------------------------------------------------

#[test]
fn test_extract_script_path_skips_interpreter_flags() {
    use sahjhan::daemon::auth::extract_script_path;

    // Python with -u flag before script
    let args = vec![
        "/usr/bin/python3".to_string(),
        "-u".to_string(),
        "-B".to_string(),
        "/path/to/hook.py".to_string(),
    ];
    assert_eq!(
        extract_script_path(&args),
        Some("/path/to/hook.py".to_string())
    );

    // Node with --experimental flag
    let args = vec![
        "/usr/local/bin/node".to_string(),
        "--experimental-modules".to_string(),
        "/path/to/hook.js".to_string(),
    ];
    assert_eq!(
        extract_script_path(&args),
        Some("/path/to/hook.js".to_string())
    );
}
