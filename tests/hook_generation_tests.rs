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
        queries: std::collections::HashMap::new(),
        batches: std::collections::HashMap::new(),
        boundaries: Vec::new(),
        attestation: Default::default(),
        lint: Default::default(),
        daemon: Default::default(),
        vault_policies: std::collections::HashMap::new(),
        trusted_callers: None,
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

    let stop = hooks.iter().find(|h| h.filename == "stop_hook.py").unwrap();
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

    let stop = hooks.iter().find(|h| h.filename == "stop_hook.py").unwrap();
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

// ---------------------------------------------------------------------------
// The wrappers, run
//
// Everything above reads the generated text. These execute it: python3 runs
// each wrapper the way Claude Code would, and the wrapper runs the sahjhan
// under test through SAHJHAN_BIN. An `allow` proves nothing here — every
// wrapper also prints one from its `except` — so each wrapper is driven to a
// *block* that only a real round trip through `hook eval` can produce.
// ---------------------------------------------------------------------------

const POST_TOOL_RULE: &str = r#"
[[hooks]]
event = "PostToolUse"
tools = ["Bash"]
action = "block"
message = "post-tool rule fired"

[hooks.check]
type = "event_count_since_last_transition"
threshold = 0
"#;

fn sahjhan_in(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sahjhan {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A minimal project in the `working` state, with its hooks generated into
/// `<dir>/hooks` — `enforcement/` is the config dir the wrappers name.
fn project_with_generated_hooks() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
        "hooks.toml",
    ] {
        std::fs::copy(
            std::path::Path::new("examples/minimal").join(file),
            config_dir.join(file),
        )
        .unwrap();
    }
    let hooks_toml = config_dir.join("hooks.toml");
    let mut text = std::fs::read_to_string(&hooks_toml).unwrap();
    text.push_str(POST_TOOL_RULE);
    std::fs::write(&hooks_toml, text).unwrap();

    sahjhan_in(dir.path(), &["init"]);
    sahjhan_in(dir.path(), &["transition", "begin"]);
    sahjhan_in(dir.path(), &["hook", "generate", "--output-dir", "hooks"]);
    dir
}

/// Run one generated wrapper on a hook event: its exit status, stderr, and
/// the JSON it printed.
fn run_wrapper(
    dir: &std::path::Path,
    script: &str,
    event: serde_json::Value,
) -> (i32, String, serde_json::Value) {
    use std::io::Write;
    let mut child = std::process::Command::new("python3")
        .arg(dir.join("hooks").join(script))
        .env("SAHJHAN_BIN", env!("CARGO_BIN_EXE_sahjhan"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("python3 is required to run the generated hooks");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let reply = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "{} printed no JSON ({}): stdout {:?}, stderr {}",
            script, e, stdout, stderr
        )
    });
    (output.status.code().unwrap(), stderr, reply)
}

#[test]
fn generated_pre_tool_hook_relays_a_block() {
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "pre_tool_hook.py",
        serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "src/main.rs"}, "cwd": cwd}),
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{}", reply);
    assert_eq!(reply["decision"], "block", "{}", reply);
    assert!(
        reply["reason"]
            .as_str()
            .unwrap()
            .contains("without a check_done event"),
        "{}",
        reply
    );
}

#[test]
fn generated_post_tool_hook_relays_a_block() {
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "post_tool_hook.py",
        serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "ls"}, "cwd": cwd}),
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{}", reply);
    assert_eq!(reply["decision"], "block", "{}", reply);
    assert_eq!(reply["reason"], "post-tool rule fired", "{}", reply);
}

#[test]
fn generated_stop_hook_relays_a_block() {
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "stop_hook.py",
        serde_json::json!({"stop_hook_output": "task complete", "cwd": cwd}),
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{}", reply);
    assert_eq!(reply["decision"], "block", "{}", reply);
    assert!(
        reply["reason"]
            .as_str()
            .unwrap()
            .contains("Cannot claim completion"),
        "{}",
        reply
    );
}
