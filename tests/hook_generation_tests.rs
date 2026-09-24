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

    // The shape Claude Code registers hooks from: each event a list of
    // matcher groups, each group a list of command handlers. Bare command
    // strings load without error and register nothing.
    let json = HookGenerator::suggested_hooks_json(&hooks, ".hooks");
    let config: serde_json::Value = serde_json::from_str(&json).unwrap();
    let commands = |event: &str| -> Vec<String> {
        let groups = config["hooks"][event].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{}: {}", event, json);
        groups[0]["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| {
                assert_eq!(h["type"], "command", "{}", json);
                h["command"].as_str().unwrap().to_string()
            })
            .collect()
    };
    let project = |file: &str| format!("python3 \"${{CLAUDE_PROJECT_DIR}}/.hooks/{}\"", file);

    assert_eq!(config["hooks"]["PreToolUse"][0]["matcher"], "*");
    assert_eq!(
        commands("PreToolUse"),
        [
            project("pre_tool_hook.py"),
            project("_sahjhan_bootstrap.py")
        ]
    );
    assert_eq!(config["hooks"]["PostToolUse"][0]["matcher"], "*");
    assert_eq!(commands("PostToolUse"), [project("post_tool_hook.py")]);
    // Stop takes no matcher.
    assert!(
        config["hooks"]["Stop"][0].get("matcher").is_none(),
        "{}",
        json
    );
    assert_eq!(commands("Stop"), [project("stop_hook.py")]);
}

#[test]
fn suggested_hooks_json_keeps_an_absolute_hooks_dir() {
    // Only a relative dir needs anchoring; an absolute one already resolves
    // from wherever Claude's tools have `cd`'d to.
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let json = HookGenerator::suggested_hooks_json(&hooks, "/opt/proj/.hooks");
    let config: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        config["hooks"]["Stop"][0]["hooks"][0]["command"],
        "python3 \"/opt/proj/.hooks/stop_hook.py\""
    );
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
        // Claude Code rejects a top-level `"decision": "allow"` as invalid
        // hook output; a wrapper allows with `{}` or a `systemMessage`.
        assert!(
            !hook.content.contains("\"decision\": \"allow\""),
            "{} must not print the allow Claude Code rejects",
            hook.filename
        );
    }
}

// ---------------------------------------------------------------------------
// The wrappers, run
//
// Everything above reads the generated text. These execute it: python3 runs
// each wrapper the way Claude Code would, and the wrapper runs the sahjhan
// under test through SAHJHAN_BIN. Each wrapper is driven to a *block*, which
// only a real round trip through `hook eval` can produce, and every reply is
// held to the shapes Claude Code acts on.
// ---------------------------------------------------------------------------

const EXTRA_RULES: &str = r#"
[[hooks]]
event = "PostToolUse"
tools = ["Bash"]
action = "block"
message = "post-tool rule fired"

[hooks.check]
type = "event_count_since_last_transition"
threshold = 0

[[hooks]]
event = "PreToolUse"
tools = ["Bash"]
action = "warn"
message = "pre-tool warning fired"

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
    text.push_str(EXTRA_RULES);
    std::fs::write(&hooks_toml, text).unwrap();

    sahjhan_in(dir.path(), &["init"]);
    sahjhan_in(dir.path(), &["transition", "begin"]);
    sahjhan_in(dir.path(), &["hook", "generate", "--output-dir", "hooks"]);
    dir
}

/// Fail unless `reply` is one of the three shapes Claude Code acts on as
/// intended — see the header of `src/hooks/generate.rs`. Any other shape is a
/// hook error on every call, or text nobody is shown.
fn assert_claude_code_shape(script: &str, reply: &serde_json::Value) {
    let obj = reply
        .as_object()
        .unwrap_or_else(|| panic!("{} printed a non-object: {}", script, reply));
    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    let acted_on = match keys.as_slice() {
        [] => true,
        ["systemMessage"] => obj["systemMessage"].is_string(),
        ["decision", "reason"] => obj["decision"] == "block" && obj["reason"].is_string(),
        _ => false,
    };
    assert!(
        acted_on,
        "{} printed a shape Claude Code does not act on: {}",
        script, reply
    );
}

/// Run one generated wrapper on a hook event: its exit status, stderr, and
/// the JSON it printed, which must be a shape Claude Code acts on.
fn run_wrapper(
    dir: &std::path::Path,
    script: &str,
    event: serde_json::Value,
) -> (i32, String, serde_json::Value) {
    run_wrapper_with_bin(dir, script, event, env!("CARGO_BIN_EXE_sahjhan"))
}

fn run_wrapper_with_bin(
    dir: &std::path::Path,
    script: &str,
    event: serde_json::Value,
    sahjhan_bin: &str,
) -> (i32, String, serde_json::Value) {
    use std::io::Write;
    let mut child = std::process::Command::new("python3")
        .arg(dir.join("hooks").join(script))
        .env("SAHJHAN_BIN", sahjhan_bin)
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
    assert_claude_code_shape(script, &reply);
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
    // The event is shaped like Claude Code's Stop input, which carries the
    // final response as `last_assistant_message` — the field the wrapper has
    // to read for an `output_contains_any` rule to see any text at all.
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "stop_hook.py",
        serde_json::json!({
            "hook_event_name": "Stop",
            "stop_hook_active": false,
            "last_assistant_message": "Task complete: everything is done.",
            "cwd": cwd,
        }),
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

/// The `systemMessage` a wrapper prints when `hook eval` did not evaluate —
/// asserting on the way that it carries no `decision`, which is how a hook
/// lets a call through without claiming any rule allowed it (#51).
fn unevaluated_notice(script: &str, code: i32, stderr: &str, reply: &serde_json::Value) -> String {
    assert_eq!((code, stderr), (0, ""), "{}: {}", script, reply);
    assert!(reply.get("decision").is_none(), "{}: {}", script, reply);
    reply["systemMessage"]
        .as_str()
        .unwrap_or_else(|| panic!("{} said nothing: {}", script, reply))
        .to_string()
}

#[test]
fn generated_hooks_say_when_sahjhan_did_not_evaluate() {
    // An unevaluated reply has no decision to relay. Each wrapper lets the
    // call through, as it does on any failure, and tells the user it went
    // unchecked — with the engine's own code, so a broken config and a
    // tampered one read differently.
    let dir = project_with_generated_hooks();
    std::fs::write(
        dir.path().join("enforcement/protocol.toml"),
        "garbage = [\n",
    )
    .unwrap();
    let cwd = dir.path().to_str().unwrap();
    for (script, event) in [
        (
            "pre_tool_hook.py",
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "src/main.rs"}, "cwd": cwd}),
        ),
        (
            "post_tool_hook.py",
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "ls"}, "cwd": cwd}),
        ),
        (
            "stop_hook.py",
            serde_json::json!({"last_assistant_message": "task complete", "cwd": cwd}),
        ),
    ] {
        let (code, stderr, reply) = run_wrapper(dir.path(), script, event);
        let notice = unevaluated_notice(script, code, &stderr, &reply);
        assert!(
            notice.contains("did not evaluate") && notice.contains("(config_error)"),
            "{}: {}",
            script,
            notice
        );
    }
}

#[test]
fn generated_hook_names_a_config_edited_after_its_seal() {
    let dir = project_with_generated_hooks();
    let states = dir.path().join("enforcement/states.toml");
    let mut text = std::fs::read_to_string(&states).unwrap();
    text.push_str("\n# edited after init\n");
    std::fs::write(&states, text).unwrap();

    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "pre_tool_hook.py",
        serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "src/main.rs"}, "cwd": cwd}),
    );
    let notice = unevaluated_notice("pre_tool_hook.py", code, &stderr, &reply);
    assert!(
        notice.contains("(integrity_error)") && notice.contains("states.toml"),
        "{}",
        notice
    );
}

#[test]
fn generated_hooks_allow_with_an_empty_object() {
    // `{}` is how a hook lets a call through. `{"decision": "allow"}` looks
    // the same and is not: Claude Code rejects it as invalid hook output and
    // records a hook error on every call it lets through.
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let read = serde_json::json!({"tool_name": "Read", "tool_input": {"file_path": "README.md"}, "cwd": cwd});
    for (script, event) in [
        ("pre_tool_hook.py", read.clone()),
        ("post_tool_hook.py", read.clone()),
        ("_sahjhan_bootstrap.py", read),
        (
            "stop_hook.py",
            serde_json::json!({"last_assistant_message": "hello", "cwd": cwd}),
        ),
    ] {
        let (code, stderr, reply) = run_wrapper(dir.path(), script, event);
        assert_eq!((code, stderr.as_str()), (0, ""), "{}: {}", script, reply);
        assert_eq!(reply, serde_json::json!({}), "{}", script);
    }
}

#[test]
fn generated_pre_tool_hook_relays_a_warning_as_a_system_message() {
    // A warning is shown to the user through `systemMessage`. A top-level
    // `message` passes Claude Code's schema and is shown to no one.
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "pre_tool_hook.py",
        serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "ls"}, "cwd": cwd}),
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{}", reply);
    assert_eq!(
        reply,
        serde_json::json!({"systemMessage": "pre-tool warning fired"})
    );
}

#[test]
fn generated_hooks_say_why_when_sahjhan_cannot_run() {
    // No binary at SAHJHAN_BIN: the wrapper's `except`. It still lets the call
    // through, and says why rather than printing an allow nobody can tell
    // from a rule that ran.
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let missing = dir.path().join("no-such-sahjhan");
    let (code, stderr, reply) = run_wrapper_with_bin(
        dir.path(),
        "pre_tool_hook.py",
        serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "src/main.rs"}, "cwd": cwd}),
        missing.to_str().unwrap(),
    );
    let notice = unevaluated_notice("pre_tool_hook.py", code, &stderr, &reply);
    assert!(
        notice.contains("did not evaluate") && notice.contains("FileNotFoundError"),
        "{}",
        notice
    );
}

#[test]
fn generated_bootstrap_blocks_a_write_to_the_enforcement_dir() {
    let dir = project_with_generated_hooks();
    let cwd = dir.path().to_str().unwrap();
    let (code, stderr, reply) = run_wrapper(
        dir.path(),
        "_sahjhan_bootstrap.py",
        serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "enforcement/protocol.toml"}, "cwd": cwd}),
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{}", reply);
    assert_eq!(reply["decision"], "block", "{}", reply);
}
