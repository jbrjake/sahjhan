// tests/hook_generation_tests.rs
//
// Integration tests for hook bridge generation (Task 10).

use sahjhan::config::{PathsConfig, ProtocolConfig, ProtocolMeta};
use sahjhan::hooks::HookGenerator;
use std::collections::HashMap;

fn make_config(managed: Vec<&str>) -> ProtocolConfig {
    ProtocolConfig {
        protocol: ProtocolMeta {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "test protocol".to_string(),
        },
        paths: PathsConfig {
            managed: managed.into_iter().map(|s| s.to_string()).collect(),
            data_dir: "output/.sahjhan".to_string(),
            render_dir: "output".to_string(),
        },
        sets: HashMap::new(),
        aliases: HashMap::new(),
        states: HashMap::new(),
        transitions: vec![],
        events: HashMap::new(),
        renders: vec![],
        checkpoints: Default::default(),
        ledgers: HashMap::new(),
        guards: None,
        hooks: vec![],
        monitors: vec![],
    }
}

#[test]
fn hook_generation_produces_valid_python() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    for hook in &hooks {
        assert!(
            hook.content.contains("json.loads"),
            "{} must contain json.loads",
            hook.filename
        );
        assert!(
            hook.content.contains("import") && hook.content.contains("json"),
            "{} must import json",
            hook.filename
        );
    }
}

#[test]
fn hook_generation_injects_managed_paths() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output", "docs/gen", "build/artifacts"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let write_guard = hooks
        .iter()
        .find(|h| h.filename == "write_guard.py")
        .expect("write_guard.py must be generated");

    assert!(write_guard.content.contains("\"output\""));
    assert!(write_guard.content.contains("\"docs/gen\""));
    assert!(write_guard.content.contains("\"build/artifacts\""));
}

#[test]
fn hook_generation_includes_bootstrap() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let bootstrap = hooks
        .iter()
        .find(|h| h.filename == "_sahjhan_bootstrap.py")
        .expect("bootstrap hook must be included");

    assert_eq!(bootstrap.hook_type, "PreToolUse");
    assert!(bootstrap.content.contains("PROTECTED"));
    assert!(bootstrap.content.contains("enforcement/"));
    assert!(bootstrap.content.contains("bin/sahjhan"));
    assert!(bootstrap.content.contains("_sahjhan_bootstrap.py"));
}

#[test]
fn hook_generation_references_config_dir() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let bash_guard = hooks
        .iter()
        .find(|h| h.filename == "bash_guard.py")
        .expect("bash_guard.py must be generated");

    assert!(
        bash_guard.content.contains("CONFIG_DIR = \"enforcement\""),
        "bash_guard must reference config dir"
    );
}

#[test]
fn hook_generation_writes_files_to_output_dir() {
    let dir = tempfile::tempdir().unwrap();
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", Some(dir.path())).unwrap();

    assert_eq!(hooks.len(), 3);

    assert!(dir.path().join("write_guard.py").exists());
    assert!(dir.path().join("bash_guard.py").exists());
    assert!(dir.path().join("_sahjhan_bootstrap.py").exists());

    // Verify file contents match returned content
    for hook in &hooks {
        let on_disk = std::fs::read_to_string(dir.path().join(&hook.filename)).unwrap();
        assert_eq!(on_disk, hook.content);
    }
}

#[test]
fn hook_generation_rejects_unknown_harness() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let result = gen.generate(&config, "vscode", None);
    assert!(result.is_err());
}

#[test]
fn hook_types_are_correct() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let wg = hooks
        .iter()
        .find(|h| h.filename == "write_guard.py")
        .unwrap();
    assert_eq!(wg.hook_type, "PreToolUse");

    let bg = hooks
        .iter()
        .find(|h| h.filename == "bash_guard.py")
        .unwrap();
    assert_eq!(bg.hook_type, "PostToolUse");

    let bs = hooks
        .iter()
        .find(|h| h.filename == "_sahjhan_bootstrap.py")
        .unwrap();
    assert_eq!(bs.hook_type, "PreToolUse");
}

#[test]
fn suggested_hooks_json_format() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let json = HookGenerator::suggested_hooks_json(&hooks, ".hooks");
    assert!(json.contains("\"PreToolUse\""));
    assert!(json.contains("\"PostToolUse\""));
    assert!(json.contains("write_guard.py"));
    assert!(json.contains("bash_guard.py"));
    assert!(json.contains("_sahjhan_bootstrap.py"));
}

#[test]
fn write_guard_blocks_write_and_edit() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let wg = hooks
        .iter()
        .find(|h| h.filename == "write_guard.py")
        .unwrap();
    // Should check for Write and Edit tool names
    assert!(wg.content.contains("\"Write\""));
    assert!(wg.content.contains("\"Edit\""));
    // Should have block decision logic
    assert!(
        wg.content.contains("\"decision\": \"block\"")
            || wg.content.contains("\"decision\": \"block\"")
    );
    assert!(wg.content.contains("WRITE BLOCKED"));
}

#[test]
fn bash_guard_runs_manifest_verify() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let bg = hooks
        .iter()
        .find(|h| h.filename == "bash_guard.py")
        .unwrap();
    assert!(bg.content.contains("manifest"));
    assert!(bg.content.contains("verify"));
    assert!(bg.content.contains("UNAUTHORIZED MODIFICATION DETECTED"));
}
