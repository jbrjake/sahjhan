// tests/hook_generation_tests.rs
//
// Integration tests for hook script generation.

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

    let pre_tool = hooks
        .iter()
        .find(|h| h.filename == "pre_tool_hook.py")
        .expect("pre_tool_hook.py must be generated");

    assert!(
        pre_tool.content.contains("CONFIG_DIR = \"enforcement\""),
        "pre_tool_hook must reference config dir"
    );

    let post_tool = hooks
        .iter()
        .find(|h| h.filename == "post_tool_hook.py")
        .expect("post_tool_hook.py must be generated");

    assert!(
        post_tool.content.contains("CONFIG_DIR = \"enforcement\""),
        "post_tool_hook must reference config dir"
    );

    let stop = hooks
        .iter()
        .find(|h| h.filename == "stop_hook.py")
        .expect("stop_hook.py must be generated");

    assert!(
        stop.content.contains("CONFIG_DIR = \"enforcement\""),
        "stop_hook must reference config dir"
    );
}

#[test]
fn hook_generation_writes_files_to_output_dir() {
    let dir = tempfile::tempdir().unwrap();
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", Some(dir.path())).unwrap();

    assert_eq!(hooks.len(), 4);

    assert!(dir.path().join("pre_tool_hook.py").exists());
    assert!(dir.path().join("post_tool_hook.py").exists());
    assert!(dir.path().join("stop_hook.py").exists());
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

    let pre = hooks
        .iter()
        .find(|h| h.filename == "pre_tool_hook.py")
        .unwrap();
    assert_eq!(pre.hook_type, "PreToolUse");

    let post = hooks
        .iter()
        .find(|h| h.filename == "post_tool_hook.py")
        .unwrap();
    assert_eq!(post.hook_type, "PostToolUse");

    let stop = hooks
        .iter()
        .find(|h| h.filename == "stop_hook.py")
        .unwrap();
    assert_eq!(stop.hook_type, "Stop");

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
    assert!(json.contains("\"Stop\""));
    assert!(json.contains("pre_tool_hook.py"));
    assert!(json.contains("post_tool_hook.py"));
    assert!(json.contains("stop_hook.py"));
    assert!(json.contains("_sahjhan_bootstrap.py"));
}

#[test]
fn thin_wrappers_delegate_to_hook_eval() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    // pre_tool_hook, post_tool_hook, stop_hook should all delegate to sahjhan hook eval
    for hook in &hooks {
        if hook.filename == "_sahjhan_bootstrap.py" {
            continue;
        }
        assert!(
            hook.content.contains("hook eval") || hook.content.contains("\"hook\", \"eval\""),
            "{} should delegate to sahjhan hook eval",
            hook.filename
        );
        assert!(
            hook.content.contains("subprocess"),
            "{} should use subprocess to call sahjhan",
            hook.filename
        );
        assert!(
            hook.content.contains("sahjhan_binary"),
            "{} should use sahjhan_binary() helper",
            hook.filename
        );
    }
}

#[test]
fn pre_tool_hook_passes_event_and_tool() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let pre = hooks
        .iter()
        .find(|h| h.filename == "pre_tool_hook.py")
        .unwrap();
    assert!(pre.content.contains("--event"));
    assert!(pre.content.contains("PreToolUse"));
    assert!(pre.content.contains("--tool"));
}

#[test]
fn post_tool_hook_passes_event_and_tool() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let post = hooks
        .iter()
        .find(|h| h.filename == "post_tool_hook.py")
        .unwrap();
    assert!(post.content.contains("--event"));
    assert!(post.content.contains("PostToolUse"));
    assert!(post.content.contains("--tool"));
}

#[test]
fn stop_hook_passes_output_text() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let stop = hooks
        .iter()
        .find(|h| h.filename == "stop_hook.py")
        .unwrap();
    assert!(stop.content.contains("--event"));
    assert!(stop.content.contains("Stop"));
    assert!(stop.content.contains("--output-text"));
}

#[test]
fn four_hooks_generated() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    assert_eq!(hooks.len(), 4);

    let filenames: Vec<&str> = hooks.iter().map(|h| h.filename.as_str()).collect();
    assert!(filenames.contains(&"pre_tool_hook.py"));
    assert!(filenames.contains(&"post_tool_hook.py"));
    assert!(filenames.contains(&"stop_hook.py"));
    assert!(filenames.contains(&"_sahjhan_bootstrap.py"));
}

#[test]
fn wrappers_fail_open_on_error() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    // All thin wrappers should have fail-open exception handling
    for hook in &hooks {
        if hook.filename == "_sahjhan_bootstrap.py" {
            continue;
        }
        assert!(
            hook.content.contains("except Exception"),
            "{} should catch exceptions for fail-open behavior",
            hook.filename
        );
        // The except block should output allow
        assert!(
            hook.content.contains("\"decision\": \"allow\""),
            "{} should default to allow on error",
            hook.filename
        );
    }
}
