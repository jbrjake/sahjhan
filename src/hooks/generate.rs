// src/hooks/generate.rs
//
// Hook script generation for Claude Code integration.
//
// ## Index
// - GeneratedHook            — hook type + script content
// - HookGenerator            — produces Python hook scripts (thin wrappers + bootstrap)

use std::path::Path;

use crate::config::ProtocolConfig;

// ---------------------------------------------------------------------------
// Embedded templates — thin wrappers that delegate to `sahjhan hook eval`.
//
// Python braces `{` / `}` are escaped as `{{` / `}}` in Rust raw strings.
// Template variables like `{config_dir}` use single braces (Rust .replace()).
// ---------------------------------------------------------------------------

const PRE_TOOL_HOOK_TEMPLATE: &str = r##"# Generated hook: pre_tool_hook.py
# PreToolUse hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{{arch}}-apple-darwin"
    else:
        triple = f"{{arch}}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{{triple}}")

CONFIG_DIR = "{config_dir}"

def main():
    try:
        event = json.loads(sys.stdin.read())
    except Exception:
        print(json.dumps({{"decision": "allow"}}))
        return

    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {{}})
    file_path = tool_input.get("file_path", tool_input.get("command", ""))

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR, "--json",
           "hook", "eval", "--event", "PreToolUse", "--tool", tool_name]
    if file_path:
        cmd.extend(["--file", file_path])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout)
        data = output.get("data", {{}})
        decision = data.get("decision", "allow")
        messages = data.get("messages", [])

        if decision == "block":
            reason = messages[0]["message"] if messages else "Blocked by protocol"
            print(json.dumps({{"decision": "block", "reason": reason}}))
        elif messages:
            combined = "\n".join(m["message"] for m in messages)
            print(json.dumps({{"decision": "allow", "message": combined}}))
        else:
            print(json.dumps({{"decision": "allow"}}))
    except Exception:
        print(json.dumps({{"decision": "allow"}}))

if __name__ == "__main__":
    main()
"##;

const POST_TOOL_HOOK_TEMPLATE: &str = r##"# Generated hook: post_tool_hook.py
# PostToolUse hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{{arch}}-apple-darwin"
    else:
        triple = f"{{arch}}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{{triple}}")

CONFIG_DIR = "{config_dir}"

def main():
    try:
        event = json.loads(sys.stdin.read())
    except Exception:
        print(json.dumps({{"decision": "allow"}}))
        return

    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {{}})
    file_path = tool_input.get("file_path", tool_input.get("command", ""))

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR, "--json",
           "hook", "eval", "--event", "PostToolUse", "--tool", tool_name]
    if file_path:
        cmd.extend(["--file", file_path])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout)
        data = output.get("data", {{}})
        decision = data.get("decision", "allow")
        messages = data.get("messages", [])

        if decision == "block":
            reason = messages[0]["message"] if messages else "Blocked by protocol"
            print(json.dumps({{"decision": "block", "reason": reason}}))
        elif messages:
            combined = "\n".join(m["message"] for m in messages)
            print(json.dumps({{"decision": "allow", "message": combined}}))
        else:
            print(json.dumps({{"decision": "allow"}}))
    except Exception:
        print(json.dumps({{"decision": "allow"}}))

if __name__ == "__main__":
    main()
"##;

const STOP_HOOK_TEMPLATE: &str = r##"# Generated hook: stop_hook.py
# Stop hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{{arch}}-apple-darwin"
    else:
        triple = f"{{arch}}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{{triple}}")

CONFIG_DIR = "{config_dir}"

def main():
    try:
        event = json.loads(sys.stdin.read())
    except Exception:
        print(json.dumps({{"decision": "allow"}}))
        return

    stop_message = event.get("stop_hook_output", event.get("stop_message", ""))

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR, "--json",
           "hook", "eval", "--event", "Stop"]
    if stop_message:
        cmd.extend(["--output-text", stop_message])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout)
        data = output.get("data", {{}})
        decision = data.get("decision", "allow")
        messages = data.get("messages", [])

        if decision == "block":
            reason = messages[0]["message"] if messages else "Blocked by protocol"
            print(json.dumps({{"decision": "block", "reason": reason}}))
        elif messages:
            combined = "\n".join(m["message"] for m in messages)
            print(json.dumps({{"decision": "allow", "message": combined}}))
        else:
            print(json.dumps({{"decision": "allow"}}))
    except Exception:
        print(json.dumps({{"decision": "allow"}}))

if __name__ == "__main__":
    main()
"##;

const BOOTSTRAP_HOOK: &str = r##"# _sahjhan_bootstrap.py — DO NOT MODIFY
# This hook protects Sahjhan's enforcement infrastructure.
# It is intentionally minimal and self-referential.
import os, sys, json

PROTECTED = ["enforcement/", "bin/sahjhan", "_sahjhan_bootstrap.py"]

event = json.loads(sys.stdin.read())
tool_name = event.get("tool_name", "")
if tool_name not in ("Write", "Edit"):
    print(json.dumps({"decision": "allow"}))
    sys.exit(0)

path = event.get("tool_input", {}).get("file_path", "")
cwd = event.get("cwd", os.getcwd())
resolved = os.path.realpath(os.path.join(cwd, path)) if path else ""

for p in PROTECTED:
    full = os.path.realpath(os.path.join(cwd, p))
    if resolved.startswith(full) or resolved == full:
        print(json.dumps({"decision": "block",
            "reason": f"BLOCKED: {path} is protected enforcement infrastructure."}))
        sys.exit(0)

print(json.dumps({"decision": "allow"}))
"##;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A generated hook script ready to be written to disk or printed.
#[derive(Debug, Clone)]
pub struct GeneratedHook {
    pub filename: String,
    pub content: String,
    pub hook_type: String, // "PreToolUse", "PostToolUse", or "Stop"
}

/// Generates Python hook scripts for Claude Code integration.
pub struct HookGenerator;

impl HookGenerator {
    pub fn new() -> Result<Self, String> {
        Ok(HookGenerator)
    }

    /// Generate hook scripts from the protocol configuration.
    ///
    /// `harness` should be `"cc"` for Claude Code (currently the only
    /// supported harness).
    ///
    /// If `output_dir` is `Some`, the generated scripts are written to
    /// that directory and file paths are returned.  Otherwise the caller
    /// is responsible for handling the content (e.g. printing to stdout).
    pub fn generate(
        &self,
        _config: &ProtocolConfig,
        harness: &str,
        output_dir: Option<&Path>,
    ) -> Result<Vec<GeneratedHook>, String> {
        if harness != "cc" {
            return Err(format!(
                "Unknown harness '{}'. Only 'cc' (Claude Code) is supported.",
                harness
            ));
        }

        let config_dir_value = "enforcement";
        let mut hooks = Vec::new();

        // --- Pre-tool hook (PreToolUse) — thin wrapper ---
        let pre_tool = PRE_TOOL_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
        hooks.push(GeneratedHook {
            filename: "pre_tool_hook.py".to_string(),
            content: pre_tool,
            hook_type: "PreToolUse".to_string(),
        });

        // --- Post-tool hook (PostToolUse) — thin wrapper ---
        let post_tool = POST_TOOL_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
        hooks.push(GeneratedHook {
            filename: "post_tool_hook.py".to_string(),
            content: post_tool,
            hook_type: "PostToolUse".to_string(),
        });

        // --- Stop hook (Stop) — thin wrapper ---
        let stop = STOP_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
        hooks.push(GeneratedHook {
            filename: "stop_hook.py".to_string(),
            content: stop,
            hook_type: "Stop".to_string(),
        });

        // --- Bootstrap hook (PreToolUse) — self-contained ---
        hooks.push(GeneratedHook {
            filename: "_sahjhan_bootstrap.py".to_string(),
            content: BOOTSTRAP_HOOK.to_string(),
            hook_type: "PreToolUse".to_string(),
        });

        // Write to disk if output_dir is specified.
        if let Some(dir) = output_dir {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Cannot create output directory {}: {}", dir.display(), e))?;

            for hook in &hooks {
                let path = dir.join(&hook.filename);
                std::fs::write(&path, &hook.content)
                    .map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
            }
        }

        Ok(hooks)
    }

    /// Return the suggested hooks.json configuration for Claude Code.
    pub fn suggested_hooks_json(hooks: &[GeneratedHook], hooks_dir: &str) -> String {
        let mut pre_hooks = Vec::new();
        let mut post_hooks = Vec::new();
        let mut stop_hooks = Vec::new();

        for hook in hooks {
            let entry = format!("\"python3 {}/{}\"", hooks_dir, hook.filename);
            match hook.hook_type.as_str() {
                "PreToolUse" => pre_hooks.push(entry),
                "PostToolUse" => post_hooks.push(entry),
                "Stop" => stop_hooks.push(entry),
                _ => {}
            }
        }

        format!(
            r#"{{
  "hooks": {{
    "PreToolUse": [
      {}
    ],
    "PostToolUse": [
      {}
    ],
    "Stop": [
      {}
    ]
  }}
}}"#,
            pre_hooks.join(",\n      "),
            post_hooks.join(",\n      "),
            stop_hooks.join(",\n      "),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PathsConfig, ProtocolConfig, ProtocolMeta};
    use std::collections::HashMap;

    fn test_config() -> ProtocolConfig {
        ProtocolConfig {
            protocol: ProtocolMeta {
                name: "test-protocol".to_string(),
                version: "1.0.0".to_string(),
                description: "Test protocol".to_string(),
            },
            paths: PathsConfig {
                managed: vec!["output".to_string(), "docs/generated".to_string()],
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
    fn test_generate_produces_valid_python() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        // All hooks should contain json.loads and sys.stdin (valid Python hook pattern)
        for hook in &hooks {
            assert!(
                hook.content.contains("json.loads"),
                "Hook {} should contain json.loads",
                hook.filename
            );
            assert!(
                hook.content.contains("sys.stdin") || hook.content.contains("sys.exit"),
                "Hook {} should reference sys module",
                hook.filename
            );
        }
    }

    #[test]
    fn test_config_dir_in_pre_tool_hook() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let pre_tool = hooks
            .iter()
            .find(|h| h.filename == "pre_tool_hook.py")
            .unwrap();
        assert!(
            pre_tool.content.contains("CONFIG_DIR = \"enforcement\""),
            "pre_tool_hook should reference config dir 'enforcement'"
        );
    }

    #[test]
    fn test_config_dir_in_post_tool_hook() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let post_tool = hooks
            .iter()
            .find(|h| h.filename == "post_tool_hook.py")
            .unwrap();
        assert!(
            post_tool.content.contains("CONFIG_DIR = \"enforcement\""),
            "post_tool_hook should reference config dir 'enforcement'"
        );
    }

    #[test]
    fn test_bootstrap_included() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let bootstrap = hooks.iter().find(|h| h.filename == "_sahjhan_bootstrap.py");
        assert!(bootstrap.is_some(), "Bootstrap hook should be included");

        let bs = bootstrap.unwrap();
        assert_eq!(bs.hook_type, "PreToolUse");
        assert!(bs.content.contains("PROTECTED"));
        assert!(bs.content.contains("enforcement/"));
    }

    #[test]
    fn test_hook_types_correct() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let pre_tool = hooks
            .iter()
            .find(|h| h.filename == "pre_tool_hook.py")
            .unwrap();
        assert_eq!(pre_tool.hook_type, "PreToolUse");

        let post_tool = hooks
            .iter()
            .find(|h| h.filename == "post_tool_hook.py")
            .unwrap();
        assert_eq!(post_tool.hook_type, "PostToolUse");

        let stop = hooks.iter().find(|h| h.filename == "stop_hook.py").unwrap();
        assert_eq!(stop.hook_type, "Stop");

        let bootstrap = hooks
            .iter()
            .find(|h| h.filename == "_sahjhan_bootstrap.py")
            .unwrap();
        assert_eq!(bootstrap.hook_type, "PreToolUse");
    }

    #[test]
    fn test_unknown_harness_rejected() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let result = gen.generate(&config, "unknown", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown harness"));
    }

    #[test]
    fn test_write_to_output_dir() {
        let dir = tempfile::tempdir().unwrap();
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", Some(dir.path())).unwrap();

        assert_eq!(hooks.len(), 4);
        for hook in &hooks {
            let path = dir.path().join(&hook.filename);
            assert!(path.exists(), "Hook file {} should exist", hook.filename);
            let content = std::fs::read_to_string(&path).unwrap();
            assert_eq!(content, hook.content);
        }
    }

    #[test]
    fn test_suggested_hooks_json() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let json = HookGenerator::suggested_hooks_json(&hooks, ".hooks");
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("PostToolUse"));
        assert!(json.contains("Stop"));
        assert!(json.contains("pre_tool_hook.py"));
        assert!(json.contains("post_tool_hook.py"));
        assert!(json.contains("stop_hook.py"));
        assert!(json.contains("_sahjhan_bootstrap.py"));
    }

    #[test]
    fn test_four_hooks_generated() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        assert_eq!(hooks.len(), 4);

        let filenames: Vec<&str> = hooks.iter().map(|h| h.filename.as_str()).collect();
        assert!(filenames.contains(&"pre_tool_hook.py"));
        assert!(filenames.contains(&"post_tool_hook.py"));
        assert!(filenames.contains(&"stop_hook.py"));
        assert!(filenames.contains(&"_sahjhan_bootstrap.py"));
    }

    #[test]
    fn test_thin_wrappers_delegate_to_hook_eval() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        for hook in &hooks {
            if hook.filename == "_sahjhan_bootstrap.py" {
                continue; // bootstrap is self-contained
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
        }
    }
}
