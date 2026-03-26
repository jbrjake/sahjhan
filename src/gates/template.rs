// src/gates/template.rs
//
// Template variable resolution with configurable escaping strategy.

use std::collections::HashMap;

/// Escaping strategy for template resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeStrategy {
    /// POSIX shell-escape: wrap in single quotes, escape interior single quotes.
    Shell,
    /// No escaping — raw substitution (for file paths, non-shell contexts).
    None,
}

/// POSIX shell-escape a string by wrapping in single quotes and replacing
/// any interior single quotes with `'\''`.
///
/// Examples:
/// ```
/// use sahjhan::gates::template::shell_escape;
/// assert_eq!(shell_escape("hello"), "'hello'");
/// assert_eq!(shell_escape("it's"), "'it'\\''s'");
/// assert_eq!(shell_escape(""), "''");
/// ```
pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Replace `{{key}}` placeholders in `template` with the corresponding
/// values from `vars`, applying the given escaping strategy.
///
/// Any placeholder whose key is not present in `vars` is left unchanged.
pub fn resolve_template_with(
    template: &str,
    vars: &HashMap<String, String>,
    strategy: EscapeStrategy,
) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let substitution = match strategy {
            EscapeStrategy::Shell => shell_escape(value),
            EscapeStrategy::None => value.clone(),
        };
        result = result.replace(&format!("{{{{{}}}}}", key), &substitution);
    }
    result
}

/// Replace `{{key}}` placeholders with shell-escaped values.
///
/// Convenience wrapper around `resolve_template_with` using `EscapeStrategy::Shell`.
pub fn resolve_template(template: &str, vars: &HashMap<String, String>) -> String {
    resolve_template_with(template, vars, EscapeStrategy::Shell)
}

/// Replace `{{key}}` placeholders with raw (unescaped) values.
///
/// Convenience wrapper around `resolve_template_with` using `EscapeStrategy::None`.
pub fn resolve_template_plain(template: &str, vars: &HashMap<String, String>) -> String {
    resolve_template_with(template, vars, EscapeStrategy::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_clean_string() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn resolve_simple_substitution() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let result = resolve_template("hello {{name}}", &vars);
        assert_eq!(result, "hello 'world'");
    }

    #[test]
    fn resolve_plain_no_escaping() {
        let mut vars = HashMap::new();
        vars.insert("path".to_string(), "/some/dir".to_string());
        let result = resolve_template_plain("{{path}}/file.txt", &vars);
        assert_eq!(result, "/some/dir/file.txt");
    }

    #[test]
    fn resolve_unknown_placeholder_unchanged() {
        let vars = HashMap::new();
        let result = resolve_template("echo {{missing}}", &vars);
        assert_eq!(result, "echo {{missing}}");
    }
}
