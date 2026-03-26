// src/gates/template.rs
//
// Template variable resolution and shell escaping for gate configs.

use std::collections::HashMap;

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
/// shell-escaped values from `vars`.
///
/// Any placeholder whose key is not present in `vars` is left unchanged.
pub fn resolve_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let escaped = shell_escape(value);
        result = result.replace(&format!("{{{{{}}}}}", key), &escaped);
    }
    result
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
    fn resolve_unknown_placeholder_unchanged() {
        let vars = HashMap::new();
        let result = resolve_template("echo {{missing}}", &vars);
        assert_eq!(result, "echo {{missing}}");
    }
}
