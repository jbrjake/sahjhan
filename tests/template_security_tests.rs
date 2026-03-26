// tests/template_security_tests.rs
//
// Security-focused tests for template variable resolution and shell escaping.

use sahjhan::gates::template::{resolve_template, shell_escape};

// ---------------------------------------------------------------------------
// shell_escape unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_shell_escape_clean_string() {
    let escaped = shell_escape("hello");
    assert_eq!(escaped, "'hello'");
}

#[test]
fn test_shell_escape_empty() {
    let escaped = shell_escape("");
    assert_eq!(escaped, "''");
}

#[test]
fn test_shell_escape_single_quotes() {
    let escaped = shell_escape("it's");
    assert_eq!(escaped, "'it'\\''s'");
}

#[test]
fn test_shell_escape_multiple_single_quotes() {
    let escaped = shell_escape("a'b'c");
    assert_eq!(escaped, "'a'\\''b'\\''c'");
}

#[test]
fn test_shell_escape_only_single_quote() {
    let escaped = shell_escape("'");
    assert_eq!(escaped, "''\\'''");
}

#[test]
fn test_shell_escape_special_chars_double_quotes() {
    // Double quotes are safe inside single-quoted strings.
    let escaped = shell_escape(r#"hello "world""#);
    assert_eq!(escaped, r#"'hello "world"'"#);
}

#[test]
fn test_shell_escape_backslash() {
    let escaped = shell_escape(r"back\slash");
    assert_eq!(escaped, r"'back\slash'");
}

#[test]
fn test_shell_escape_dollar_sign() {
    // $ has no special meaning inside single quotes.
    let escaped = shell_escape("$HOME");
    assert_eq!(escaped, "'$HOME'");
}

#[test]
fn test_shell_escape_backtick() {
    let escaped = shell_escape("`whoami`");
    assert_eq!(escaped, "'`whoami`'");
}

#[test]
fn test_shell_escape_newline() {
    let escaped = shell_escape("line1\nline2");
    assert_eq!(escaped, "'line1\nline2'");
}

// ---------------------------------------------------------------------------
// resolve_template unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_shell_metacharacters_escaped() {
    let result = resolve_template(
        "grep -q {{id}}",
        &[("id".to_string(), "'; rm -rf /; echo '".to_string())]
            .into_iter()
            .collect(),
    );
    // The injected value should be wrapped in single quotes, neutralizing the
    // attempt to break out of the argument.  The shell_escape function wraps
    // the value in single-quotes and replaces interior `'` with `'\''`, so the
    // dangerous sequences are inside the literal argument, not shell metacharacters.
    // Verify that the unsafe shell metacharacters `;` and the closing `'` used
    // to escape the quoting context are NOT present as bare characters outside
    // of the single-quoted wrapper.  The simplest invariant: the resolved string
    // must start with a single-quote-wrapped form.
    assert!(result.contains("'\\''"), "single-quote escape should be present");
    // Also verify the overall structure: our resolved value is single-quoted.
    let expected_value = shell_escape("'; rm -rf /; echo '");
    assert!(result.contains(&expected_value));
}

#[test]
fn test_valid_value_passes_through() {
    let result = resolve_template(
        "grep -q {{id}}",
        &[("id".to_string(), "BH-001".to_string())]
            .into_iter()
            .collect(),
    );
    assert!(result.contains("BH-001"));
}

#[test]
fn test_resolve_multiple_variables() {
    let vars = [
        ("foo".to_string(), "bar".to_string()),
        ("baz".to_string(), "qux".to_string()),
    ]
    .into_iter()
    .collect();
    let result = resolve_template("{{foo}} and {{baz}}", &vars);
    assert!(result.contains("'bar'"));
    assert!(result.contains("'qux'"));
}

#[test]
fn test_resolve_unknown_placeholder_unchanged() {
    let vars = std::collections::HashMap::new();
    let result = resolve_template("echo {{missing}}", &vars);
    assert_eq!(result, "echo {{missing}}");
}

#[test]
fn test_resolve_empty_value() {
    let vars = [("key".to_string(), "".to_string())]
        .into_iter()
        .collect();
    let result = resolve_template("cmd {{key}}", &vars);
    assert_eq!(result, "cmd ''");
}

// ---------------------------------------------------------------------------
// E5: Integration test — template injection attempt via gate evaluation
// ---------------------------------------------------------------------------

#[test]
fn test_command_gate_with_injection_attempt() {
    use sahjhan::gates::evaluator::{evaluate_gate, GateContext};
    use sahjhan::config::{GateConfig, ProtocolConfig};
    use sahjhan::ledger::chain::Ledger;
    use tempfile::tempdir;
    use std::collections::HashMap;
    use std::path::Path;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Create a marker file path that the injection would create if it succeeded.
    let marker = dir.path().join("pwned");

    // Template with a variable that contains an injection attempt.
    let mut state_params = HashMap::new();
    state_params.insert(
        "id".to_string(),
        format!("'; touch {}; echo '", marker.display()),
    );

    let gate = GateConfig {
        gate_type: "command_succeeds".to_string(),
        params: [("cmd".to_string(), toml::Value::String("echo {{id}}".to_string()))]
            .into_iter()
            .collect(),
    };

    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    // Evaluate the gate — it should NOT create the marker file.
    let _result = evaluate_gate(&gate, &ctx);
    assert!(
        !marker.exists(),
        "Shell injection succeeded — marker file was created!"
    );
}

#[test]
fn test_injection_via_semicolon() {
    use sahjhan::gates::evaluator::{evaluate_gate, GateContext};
    use sahjhan::config::{GateConfig, ProtocolConfig};
    use sahjhan::ledger::chain::Ledger;
    use tempfile::tempdir;
    use std::collections::HashMap;
    use std::path::Path;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let marker = dir.path().join("injected");

    let mut state_params = HashMap::new();
    state_params.insert(
        "val".to_string(),
        format!("x; touch {}", marker.display()),
    );

    let gate = GateConfig {
        gate_type: "command_succeeds".to_string(),
        params: [("cmd".to_string(), toml::Value::String("echo {{val}}".to_string()))]
            .into_iter()
            .collect(),
    };

    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let _result = evaluate_gate(&gate, &ctx);
    assert!(
        !marker.exists(),
        "Semicolon injection succeeded — marker file was created!"
    );
}

#[test]
fn test_injection_via_backtick() {
    use sahjhan::gates::evaluator::{evaluate_gate, GateContext};
    use sahjhan::config::{GateConfig, ProtocolConfig};
    use sahjhan::ledger::chain::Ledger;
    use tempfile::tempdir;
    use std::collections::HashMap;
    use std::path::Path;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let marker = dir.path().join("backtick_injected");

    let mut state_params = HashMap::new();
    state_params.insert(
        "val".to_string(),
        format!("`touch {}`", marker.display()),
    );

    let gate = GateConfig {
        gate_type: "command_succeeds".to_string(),
        params: [("cmd".to_string(), toml::Value::String("echo {{val}}".to_string()))]
            .into_iter()
            .collect(),
    };

    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let _result = evaluate_gate(&gate, &ctx);
    assert!(
        !marker.exists(),
        "Backtick injection succeeded — marker file was created!"
    );
}

#[test]
fn test_injection_via_dollar_parens() {
    use sahjhan::gates::evaluator::{evaluate_gate, GateContext};
    use sahjhan::config::{GateConfig, ProtocolConfig};
    use sahjhan::ledger::chain::Ledger;
    use tempfile::tempdir;
    use std::collections::HashMap;
    use std::path::Path;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let marker = dir.path().join("dollar_injected");

    let mut state_params = HashMap::new();
    state_params.insert(
        "val".to_string(),
        format!("$(touch {})", marker.display()),
    );

    let gate = GateConfig {
        gate_type: "command_succeeds".to_string(),
        params: [("cmd".to_string(), toml::Value::String("echo {{val}}".to_string()))]
            .into_iter()
            .collect(),
    };

    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let _result = evaluate_gate(&gate, &ctx);
    assert!(
        !marker.exists(),
        "$(…) injection succeeded — marker file was created!"
    );
}
