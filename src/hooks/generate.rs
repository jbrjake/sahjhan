// src/hooks/generate.rs
//
// Hook script generation for Claude Code integration.
//
// ## Index
// - GeneratedHook            — hook type + script content
// - HookGenerator            — produces Python hook scripts for write protection

use std::path::Path;

use crate::config::ProtocolConfig;

// ---------------------------------------------------------------------------
// Embedded templates — simple string-based injection rather than full Tera,
// since the template logic is just inserting managed paths and config dir.
// ---------------------------------------------------------------------------

const WRITE_GUARD_TEMPLATE: &str = r##"# Generated hook: write_guard.py
# Write protection for managed paths — PreToolUse hook
import os, sys, json

MANAGED = {managed_paths}  # Injected from protocol.toml at generation time

def main():
    event = json.loads(sys.stdin.read())
    tool_name = event.get("tool_name", "")
    if tool_name not in ("Write", "Edit"):
        print(json.dumps({{"decision": "allow"}}))
        return

    tool_input = event.get("tool_input", {{}})
    file_path = tool_input.get("file_path", "")
    cwd = event.get("cwd", os.getcwd())
    resolved = os.path.realpath(os.path.join(cwd, file_path))

    for prefix in MANAGED:
        managed_abs = os.path.realpath(os.path.join(cwd, prefix))
        if resolved.startswith(managed_abs + os.sep) or resolved == managed_abs:
            print(json.dumps({{
                "decision": "block",
                "reason": f"WRITE BLOCKED: {{file_path}} is managed by sahjhan. "
                          f"Use CLI commands to modify protocol state. "
                          f"Direct writes are not permitted."
            }}))
            return

    print(json.dumps({{"decision": "allow"}}))

if __name__ == "__main__":
    main()
"##;

const BASH_GUARD_TEMPLATE: &str = r##"# Generated hook: bash_guard.py
# Post-execution manifest verification — PostToolUse hook
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

CONFIG_DIR = "{config_dir}"  # Injected from protocol config

def main():
    event = json.loads(sys.stdin.read())
    tool_name = event.get("tool_name", "")
    if tool_name != "Bash":
        print(json.dumps({{"decision": "allow"}}))
        return

    cwd = event.get("cwd", os.getcwd())
    result = subprocess.run(
        [sahjhan_binary(), "--config-dir", CONFIG_DIR, "manifest", "verify"],
        capture_output=True, text=True, cwd=cwd, timeout=30,
    )
    if result.returncode != 0:
        # Record violation
        subprocess.run(
            [sahjhan_binary(), "--config-dir", CONFIG_DIR, "event",
             "protocol_violation", "--field", f"detail={{result.stdout.strip()}}"],
            cwd=cwd, timeout=10,
        )
        print(json.dumps({{
            "decision": "allow",
            "message": f"UNAUTHORIZED MODIFICATION DETECTED:\n{{result.stdout.strip()}}\n"
                       f"This violation has been recorded in the ledger."
        }}))
        return

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
    pub hook_type: String, // "PreToolUse" or "PostToolUse"
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
        config: &ProtocolConfig,
        harness: &str,
        output_dir: Option<&Path>,
    ) -> Result<Vec<GeneratedHook>, String> {
        if harness != "cc" {
            return Err(format!(
                "Unknown harness '{}'. Only 'cc' (Claude Code) is supported.",
                harness
            ));
        }

        let mut hooks = Vec::new();

        // --- Write guard (PreToolUse) ---
        let managed_paths = format!(
            "[{}]",
            config
                .paths
                .managed
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let write_guard = WRITE_GUARD_TEMPLATE.replace("{managed_paths}", &managed_paths);
        hooks.push(GeneratedHook {
            filename: "write_guard.py".to_string(),
            content: write_guard,
            hook_type: "PreToolUse".to_string(),
        });

        // --- Bash guard (PostToolUse) ---
        // The config_dir for the bash guard is the directory name the CLI
        // uses (typically "enforcement"), extracted from wherever the config
        // was loaded.  We use the protocol name's conventional directory.
        let config_dir_value = "enforcement";
        let bash_guard = BASH_GUARD_TEMPLATE.replace("{config_dir}", config_dir_value);
        hooks.push(GeneratedHook {
            filename: "bash_guard.py".to_string(),
            content: bash_guard,
            hook_type: "PostToolUse".to_string(),
        });

        // --- Bootstrap hook (PreToolUse) ---
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

        for hook in hooks {
            let entry = format!("\"python3 {}/{}\"", hooks_dir, hook.filename);
            match hook.hook_type.as_str() {
                "PreToolUse" => pre_hooks.push(entry),
                "PostToolUse" => post_hooks.push(entry),
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
    ]
  }}
}}"#,
            pre_hooks.join(",\n      "),
            post_hooks.join(",\n      "),
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
    fn test_managed_paths_injected() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let write_guard = hooks
            .iter()
            .find(|h| h.filename == "write_guard.py")
            .unwrap();
        assert!(
            write_guard.content.contains("\"output\""),
            "write_guard should contain managed path 'output'"
        );
        assert!(
            write_guard.content.contains("\"docs/generated\""),
            "write_guard should contain managed path 'docs/generated'"
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
    fn test_config_dir_in_bash_guard() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let bash_guard = hooks
            .iter()
            .find(|h| h.filename == "bash_guard.py")
            .unwrap();
        assert!(
            bash_guard.content.contains("CONFIG_DIR = \"enforcement\""),
            "bash_guard should reference config dir 'enforcement'"
        );
    }

    #[test]
    fn test_hook_types_correct() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        let write_guard = hooks
            .iter()
            .find(|h| h.filename == "write_guard.py")
            .unwrap();
        assert_eq!(write_guard.hook_type, "PreToolUse");

        let bash_guard = hooks
            .iter()
            .find(|h| h.filename == "bash_guard.py")
            .unwrap();
        assert_eq!(bash_guard.hook_type, "PostToolUse");
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

        assert_eq!(hooks.len(), 3);
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
        assert!(json.contains("write_guard.py"));
        assert!(json.contains("bash_guard.py"));
        assert!(json.contains("_sahjhan_bootstrap.py"));
    }

    #[test]
    fn test_three_hooks_generated() {
        let gen = HookGenerator::new().unwrap();
        let config = test_config();
        let hooks = gen.generate(&config, "cc", None).unwrap();

        assert_eq!(hooks.len(), 3);

        let filenames: Vec<&str> = hooks.iter().map(|h| h.filename.as_str()).collect();
        assert!(filenames.contains(&"write_guard.py"));
        assert!(filenames.contains(&"bash_guard.py"));
        assert!(filenames.contains(&"_sahjhan_bootstrap.py"));
    }
}
