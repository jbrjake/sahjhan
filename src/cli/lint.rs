// src/cli/lint.rs
//
// Static integrity analysis command.
//
// ## Index
// - [cmd-lint] cmd_lint() — run the lint checks over a protocol config

use crate::lint::{self, LintOptions, Severity};

use super::commands::{resolve_config_dir, EXIT_CONFIG_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR};
use super::output::{CommandOutput, CommandResult, LintData};

// [cmd-lint]
/// Analyze the protocol graph for integrity defects.
///
/// Config-only: no ledger is opened and no gate command runs, so this is safe
/// to wire into a pre-commit hook. Exits non-zero when any error-severity
/// finding is reported, or when `strict` is set and there are warnings.
pub fn cmd_lint(config_dir: &str, only: &[String], strict: bool) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);

    let unknown = lint::unknown_check_ids(only);
    if !unknown.is_empty() {
        let known: Vec<&str> = lint::CHECKS.iter().map(|(id, _)| *id).collect();
        return Box::new(CommandResult::<LintData>::err(
            "lint",
            EXIT_USAGE_ERROR,
            "unknown_check",
            format!(
                "unknown check id(s): {} (known: {})",
                unknown.join(", "),
                known.join(", ")
            ),
        ));
    }

    // Lint reads the config directly rather than through load_config: a
    // protocol that fails structural validation should still be lintable, and
    // reporting both sets of problems at once is more useful than stopping.
    let config = match crate::config::ProtocolConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            return Box::new(CommandResult::<LintData>::err(
                "lint",
                EXIT_CONFIG_ERROR,
                "config_error",
                e,
            ))
        }
    };

    let opts = LintOptions {
        only: only.to_vec(),
    };
    let findings = lint::run(&config, &opts);

    let error_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warning_count = findings.len() - error_count;

    let checks_run: Vec<String> = lint::CHECKS
        .iter()
        .map(|(id, _)| (*id).to_string())
        .filter(|id| {
            if only.is_empty() {
                !config
                    .lint
                    .disabled_checks
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(id))
            } else {
                only.iter().any(|o| o.eq_ignore_ascii_case(id))
            }
        })
        .collect();

    let exit_code = if error_count > 0 || (strict && warning_count > 0) {
        EXIT_CONFIG_ERROR
    } else {
        EXIT_SUCCESS
    };

    Box::new(CommandResult::ok_with_exit_code(
        "lint",
        LintData {
            findings,
            error_count,
            warning_count,
            checks_run,
        },
        exit_code,
    ))
}
